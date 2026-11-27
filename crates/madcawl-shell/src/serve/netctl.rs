//! The host's end of the network service's control channel, and reading
//! and writing the service's frames with ids and deadlines.

use std::collections::VecDeque;
use std::io::{self, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

use madcawl_net::wire::{Frame, FrameError, MAX_FRAME, kind, tag};

/// The host's end of the control channel. Each request has an id of its
/// own and only the reply with that id answers it: a reply that comes
/// after its request gave up would otherwise answer the next request,
/// handing one session another's registration. A registration whose
/// request gave up is dropped when its reply comes, and a release with
/// no time left is sent without waiting, or, when the channel was busy,
/// ahead of the next request, so that neither stays with the service.
pub(super) struct Control {
    s: UnixStream,
    buf: Vec<u8>,
    next_id: u64,
    /// Which network process the channel is to, counting from 0
    pub generation: u64,
    pub braked: bool,
    /// Requests to register that gave up before their reply came
    abandoned: VecDeque<u64>,
    /// Registrations to drop ahead of the next request
    unsent: Vec<Vec<u8>>,
}

/// How many given-up registrations and unsent releases are kept; past
/// that the oldest is let go, and the service keeps it until it ends.
const KEPT: usize = 64;

fn timed_out(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, why.to_owned())
}

impl Control {
    pub fn new(s: UnixStream, wait: Duration) -> io::Result<Control> {
        s.set_write_timeout(Some(wait))?;
        Ok(Control { s, buf: Vec::new(), next_id: 0, generation: 0, braked: false, abandoned: VecDeque::new(), unsent: Vec::new() })
    }

    /// Sends `f` and waits at most `wait` for its reply. With no time
    /// left a release is sent unwaited and anything else not at all: its
    /// reply would come to no one.
    pub fn request(&mut self, f: Frame, wait: Duration) -> io::Result<Frame> {
        for t in std::mem::take(&mut self.unsent) {
            self.send(Frame::new(kind::UNREGISTER).bytes(tag::TOKEN, &t))?;
        }
        if wait.is_zero() {
            if f.kind == kind::UNREGISTER {
                self.send(f)?;
            }
            return Err(timed_out("no time was left to wait for the network service"));
        }
        let register = f.kind == kind::REGISTER;
        let id = self.send(f)?;
        let deadline = Instant::now() + wait;
        loop {
            match read_frame(&mut self.s, &mut self.buf, deadline)? {
                Some(r) if r.get_num(tag::ID) == Some(id) => return Ok(r),
                Some(r) => self.late(&r)?,
                None => {
                    if register {
                        push_kept(&mut self.abandoned, id);
                    }
                    return Err(timed_out("the network service did not answer in time"));
                }
            }
        }
    }

    fn send(&mut self, f: Frame) -> io::Result<u64> {
        self.next_id += 1;
        write_with_id(&mut self.s, f, self.next_id)?;
        Ok(self.next_id)
    }

    /// A reply whose request gave up: a registration made for it is
    /// dropped at once.
    fn late(&mut self, r: &Frame) -> io::Result<()> {
        let pos = r.get_num(tag::ID).and_then(|id| self.abandoned.iter().position(|&a| a == id));
        let Some(pos) = pos else { return Ok(()) };
        self.abandoned.remove(pos);
        match (r.kind, r.field(tag::TOKEN)) {
            (kind::REGISTERED, Some(t)) => self.send(Frame::new(kind::UNREGISTER).bytes(tag::TOKEN, t)).map(drop),
            _ => Ok(()),
        }
    }
}

fn keep_release(unsent: &mut Vec<Vec<u8>>, f: &Frame) {
    if let (kind::UNREGISTER, Some(t)) = (f.kind, f.field(tag::TOKEN)) {
        push_release(unsent, t.to_vec());
    }
}

fn push_release(unsent: &mut Vec<Vec<u8>>, t: Vec<u8>) {
    if unsent.len() == KEPT {
        unsent.remove(0);
    }
    unsent.push(t);
}

fn push_kept(q: &mut VecDeque<u64>, id: u64) {
    if q.len() == KEPT {
        q.pop_front();
    }
    q.push_back(id);
}

/// The control channel as the host's threads share it, and the releases
/// that gave up waiting for it, handed to it the next time it is taken.
pub(super) struct ControlLock(Mutex<Control>, Mutex<Vec<Vec<u8>>>);

impl ControlLock {
    pub fn new(c: Control) -> ControlLock {
        ControlLock(Mutex::new(c), Mutex::default())
    }

    pub fn lock(&self) -> MutexGuard<'_, Control> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The channel for request `f`, waited for until `by` when there is
    /// one; a release that gave up waiting is kept to send later.
    pub fn lock_by(&self, by: Option<Instant>, f: &Frame) -> io::Result<MutexGuard<'_, Control>> {
        let mut c = match by {
            None => self.lock(),
            Some(by) => loop {
                match self.0.try_lock() {
                    Ok(c) => break c,
                    Err(TryLockError::Poisoned(e)) => break e.into_inner(),
                    Err(TryLockError::WouldBlock) if Instant::now() >= by => {
                        keep_release(&mut self.1.lock().unwrap_or_else(|e| e.into_inner()), f);
                        return Err(timed_out("the network service's channel was busy"));
                    }
                    Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(1)),
                }
            },
        };
        for t in self.1.lock().unwrap_or_else(|e| e.into_inner()).drain(..) {
            push_release(&mut c.unsent, t);
        }
        Ok(c)
    }
}

/// One page's connection to the network service: frames out, and frames
/// in as they arrive, kept whole across reads that time out.
pub struct NetConn {
    s: UnixStream,
    buf: Vec<u8>,
    pub(super) token: Vec<u8>,
    next_id: u64,
}

impl NetConn {
    /// Connects to the service at `socket` and opens the connection with
    /// HELLO and the registration `token`, waiting until `by` for the
    /// service to take it.
    pub(super) fn open(socket: &Path, token: Vec<u8>, write_wait: Duration, by: Instant) -> io::Result<NetConn> {
        let mut s = UnixStream::connect(socket)?;
        s.set_write_timeout(Some(write_wait))?;
        let mut conn = NetConn { s: s.try_clone()?, buf: Vec::new(), token, next_id: 1 };
        Frame::new(kind::HELLO).num(tag::ID, 0).bytes(tag::TOKEN, &conn.token).write_to(&mut s)?;
        match conn.next(by)? {
            Some(f) if f.kind == kind::HELLO && f.get_num(tag::OK) == Some(1) => Ok(conn),
            Some(_) => Err(io::Error::other("the network service refused the host's connection")),
            None => Err(io::Error::new(io::ErrorKind::TimedOut, "the network service did not take the connection in time")),
        }
    }

    /// Sends a FETCH; its id.
    pub fn fetch(&mut self, f: Frame) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        write_with_id(&mut self.s, f, id)?;
        Ok(id)
    }

    /// The next frame, or `None` once `deadline` passes with no whole
    /// frame in.
    pub fn next(&mut self, deadline: Instant) -> io::Result<Option<Frame>> {
        read_frame(&mut self.s, &mut self.buf, deadline)
    }
}

/// Sends `f` with `id` as its only id.
pub(super) fn write_with_id(s: &mut UnixStream, mut f: Frame, id: u64) -> io::Result<()> {
    f.fields.retain(|(t, _)| *t != tag::ID);
    f.fields.insert(0, (tag::ID, varint(id)));
    let mut out = Vec::new();
    f.encode(&mut out);
    s.write_all(&out)
}

/// The next whole frame on `s`, kept in `buf` across reads that time
/// out, or `None` once `deadline` passes without one.
pub(super) fn read_frame(s: &mut UnixStream, buf: &mut Vec<u8>, deadline: Instant) -> io::Result<Option<Frame>> {
    loop {
        match Frame::decode(buf) {
            Ok((f, used)) => {
                buf.drain(..used);
                return Ok(Some(f));
            }
            Err(FrameError::Short) => {}
            Err(e) => return Err(io::Error::other(format!("a bad frame from the network service: {e:?}"))),
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(None);
        }
        match s.set_read_timeout(Some(left.max(Duration::from_millis(1)))) {
            // macos rejects it once the peer has closed; read without waiting
            Err(e) if e.kind() == io::ErrorKind::InvalidInput => s.set_nonblocking(true)?,
            r => r?,
        }
        let mut chunk = [0u8; 64 << 10];
        match s.read(&mut chunk) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) if buf.len() + n > MAX_FRAME + 4 => {
                return Err(io::Error::other("a frame longer than the network service sends"));
            }
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => return Ok(None),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

fn varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_late_reply_does_not_answer_the_next_request() {
        let (host, mut peer) = UnixStream::pair().unwrap();
        let mut c = Control::new(host, Duration::from_secs(1)).unwrap();
        let (timed_out_tx, timed_out_rx) = std::sync::mpsc::channel();
        // the peer answers the first request only once it has timed out,
        // so the test holds however slowly either thread is scheduled
        let slow = std::thread::spawn(move || {
            let first = Frame::read_from(&mut peer).unwrap().unwrap();
            timed_out_rx.recv().unwrap();
            let late = Frame::new(kind::REGISTERED).num(tag::ID, first.get_num(tag::ID).unwrap()).bytes(tag::TOKEN, b"session-x-token!");
            late.write_to(&mut peer).unwrap();
            let second = Frame::read_from(&mut peer).unwrap().unwrap();
            let mine = Frame::new(kind::REGISTERED).num(tag::ID, second.get_num(tag::ID).unwrap()).bytes(tag::TOKEN, b"session-y-token!");
            mine.write_to(&mut peer).unwrap();
            // the late registration is dropped as the second request reads past it
            let dropped = Frame::read_from(&mut peer).unwrap().unwrap();
            assert_eq!((dropped.kind, dropped.field(tag::TOKEN)), (kind::UNREGISTER, Some(&b"session-x-token!"[..])));
        });
        let reg = || Frame::new(kind::REGISTER).num(tag::NAVIGATION, 1).num(tag::ID, 1);
        let timed_out = c.request(reg(), Duration::from_secs(1)).unwrap_err();
        assert_eq!(timed_out.kind(), io::ErrorKind::TimedOut);
        timed_out_tx.send(()).unwrap();
        // the late reply is on the stream ahead of the second one
        let r = c.request(reg(), Duration::from_secs(1)).unwrap();
        assert_eq!(r.field(tag::TOKEN), Some(&b"session-y-token!"[..]));
        slow.join().unwrap();
    }

    #[test]
    fn a_registration_with_no_time_left_is_not_asked_for_and_a_late_one_is_dropped() {
        let (host, mut peer) = UnixStream::pair().unwrap();
        let mut c = Control::new(host, Duration::from_secs(1)).unwrap();
        let reg = || Frame::new(kind::REGISTER).num(tag::NAVIGATION, 1);
        assert_eq!(c.request(reg(), Duration::ZERO).unwrap_err().kind(), io::ErrorKind::TimedOut);
        let unreg = Frame::new(kind::UNREGISTER).bytes(tag::TOKEN, b"released-late!!!");
        assert_eq!(c.request(unreg, Duration::ZERO).unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert_eq!(c.request(reg(), Duration::from_millis(50)).unwrap_err().kind(), io::ErrorKind::TimedOut);
        // the release is sent unwaited, the register with no time not at all
        let mut buf = Vec::new();
        let mut next = || read_frame(&mut peer, &mut buf, Instant::now() + Duration::from_secs(1)).unwrap().unwrap();
        let first = next();
        assert_eq!((first.kind, first.field(tag::TOKEN)), (kind::UNREGISTER, Some(&b"released-late!!!"[..])));
        let given_up = next();
        assert_eq!(given_up.kind, kind::REGISTER);
        let late = Frame::new(kind::REGISTERED).num(tag::ID, given_up.get_num(tag::ID).unwrap()).bytes(tag::TOKEN, b"nobody-has-this!");
        late.write_to(&mut peer).unwrap();
        let answer = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let by = || Instant::now() + Duration::from_secs(1);
            let third = read_frame(&mut peer, &mut buf, by()).unwrap().unwrap();
            Frame::new(kind::REGISTERED).num(tag::ID, third.get_num(tag::ID).unwrap()).bytes(tag::TOKEN, b"session-z-token!").write_to(&mut peer).unwrap();
            let dropped = read_frame(&mut peer, &mut buf, by()).unwrap().unwrap();
            (dropped.kind, dropped.field(tag::TOKEN).map(<[u8]>::to_vec))
        });
        let r = c.request(reg(), Duration::from_secs(1)).unwrap();
        assert_eq!(r.field(tag::TOKEN), Some(&b"session-z-token!"[..]));
        assert_eq!(answer.join().unwrap(), (kind::UNREGISTER, Some(b"nobody-has-this!".to_vec())), "the late registration is dropped");
    }

    #[test]
    fn a_release_that_cannot_wait_for_a_busy_channel_is_sent_with_the_next_request() {
        let (host, mut peer) = UnixStream::pair().unwrap();
        let lock = ControlLock::new(Control::new(host, Duration::from_secs(1)).unwrap());
        let unreg = Frame::new(kind::UNREGISTER).bytes(tag::TOKEN, b"released-busy!!!");
        let held = lock.lock();
        let t0 = Instant::now();
        let e = std::thread::scope(|s| s.spawn(|| lock.lock_by(Some(Instant::now() + Duration::from_millis(50)), &unreg).err()).join().unwrap());
        assert_eq!(e.map(|e| e.kind()), Some(io::ErrorKind::TimedOut));
        assert!(t0.elapsed() < Duration::from_secs(1), "{:?}", t0.elapsed());
        drop(held);
        let mut c = lock.lock_by(None, &Frame::new(kind::REGISTER)).unwrap();
        assert_eq!(c.request(Frame::new(kind::REGISTER), Duration::from_millis(20)).unwrap_err().kind(), io::ErrorKind::TimedOut);
        let first = read_frame(&mut peer, &mut Vec::new(), Instant::now() + Duration::from_secs(1)).unwrap().unwrap();
        assert_eq!((first.kind, first.field(tag::TOKEN)), (kind::UNREGISTER, Some(&b"released-busy!!!"[..])));
    }

    #[test]
    fn a_reply_written_just_before_the_service_closed_is_read() {
        let (mut host, mut peer) = UnixStream::pair().unwrap();
        Frame::new(kind::REGISTERED).num(tag::ID, 7).write_to(&mut peer).unwrap();
        drop(peer);
        let mut buf = Vec::new();
        let f = read_frame(&mut host, &mut buf, Instant::now() + Duration::from_secs(1)).unwrap().unwrap();
        assert_eq!(f.get_num(tag::ID), Some(7));
    }
}
