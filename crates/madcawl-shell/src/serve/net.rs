//! The host's network: the network service (`madcawl-netd`'s guarded
//! service, run on a thread of this process), the control channel the
//! host registers on, and a connection per page. A page in this host is
//! the host's own: its navigations and the requests of its document go
//! on a connection registered for the host, which the service lets
//! through as sent. Each agent session's pages are in a cookie partition
//! of the session's own, opened with its first web page and dropped when
//! the session ends; partitions live in memory only and the service has
//! no profile, so an agent's pages never carry the user's logins, nor
//! another session's.

use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::FileTypeExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use madcawl_net::wire::{Frame, FrameError, MAX_FRAME, kind, tag};
use madcawl_net::{Client, Config};
use madcawl_netd::{Downloads, ServerLimits, serve_guarded};

/// How long a control-channel answer or a write to the service may take.
const CONTROL_WAIT: Duration = Duration::from_secs(10);

pub struct Net {
    socket: PathBuf,
    control: Mutex<Control>,
    pub downloads: Option<PathBuf>,
}

/// The host's end of the control channel. Each request has an id of its
/// own and only the reply with that id answers it: a reply that comes
/// after its request gave up would otherwise answer the next request,
/// handing one session another's registration.
struct Control {
    s: UnixStream,
    buf: Vec<u8>,
    next_id: u64,
    wait: Duration,
}

impl Control {
    fn new(s: UnixStream, wait: Duration) -> io::Result<Control> {
        s.set_write_timeout(Some(wait))?;
        Ok(Control { s, buf: Vec::new(), next_id: 0, wait })
    }

    fn request(&mut self, f: Frame) -> io::Result<Frame> {
        self.next_id += 1;
        let id = self.next_id;
        write_with_id(&mut self.s, f, id)?;
        let deadline = Instant::now() + self.wait;
        loop {
            match read_frame(&mut self.s, &mut self.buf, deadline)? {
                Some(r) if r.get_num(tag::ID) == Some(id) => return Ok(r),
                Some(_) => {}
                None => return Err(io::Error::new(io::ErrorKind::TimedOut, "the network service did not answer in time")),
            }
        }
    }
}

/// Sends `f` with `id` as its only id.
fn write_with_id(s: &mut UnixStream, mut f: Frame, id: u64) -> io::Result<()> {
    f.fields.retain(|(t, _)| *t != tag::ID);
    f.fields.insert(0, (tag::ID, varint(id)));
    let mut out = Vec::new();
    f.encode(&mut out);
    s.write_all(&out)
}

/// The next whole frame on `s`, kept in `buf` across reads that time
/// out, or `None` once `deadline` passes without one.
fn read_frame(s: &mut UnixStream, buf: &mut Vec<u8>, deadline: Instant) -> io::Result<Option<Frame>> {
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
        s.set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
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

/// A session's cookie partition; never 0, which is the profile's.
pub(crate) fn partition_of(session: u32) -> u64 {
    u64::from(session) + 1
}

impl Net {
    /// Starts the service on a socket at `socket`, saving downloads into
    /// `downloads` (each file at most `per_file` bytes, the directory at
    /// most `total`), with at most `sessions` cookie partitions open.
    pub fn start(socket: &Path, downloads: Option<(&Path, u64, u64)>, sessions: usize) -> io::Result<Net> {
        if std::fs::symlink_metadata(socket).is_ok_and(|m| m.file_type().is_socket()) {
            std::fs::remove_file(socket)?;
        }
        let listener = UnixListener::bind(socket)?;
        let (host, theirs) = UnixStream::pair()?;
        let control = Control::new(host, CONTROL_WAIT)?;
        let d = match downloads {
            Some((dir, per_file, total)) => {
                std::fs::create_dir_all(dir)?;
                Some(Arc::new(Downloads::with_caps(dir.to_path_buf(), per_file, total, 10_000)))
            }
            None => None,
        };
        let client = Arc::new(Client::new(Config { partitions: sessions, ..Config::default() }));
        std::thread::Builder::new().name("netd".into()).spawn(move || {
            if let Err(e) = serve_guarded(listener, theirs, client, ServerLimits::default(), d) {
                eprintln!("madcawl-shell: network service: {e}");
            }
        })?;
        Ok(Net { socket: socket.to_path_buf(), control: Mutex::new(control), downloads: downloads.map(|d| d.0.to_path_buf()) })
    }

    fn control(&self, f: Frame) -> io::Result<Frame> {
        self.control.lock().unwrap_or_else(|e| e.into_inner()).request(f)
    }

    /// A connection of the host's own, for one page of agent session
    /// `session`, in that session's cookie partition.
    pub fn connect(&self, session: u32) -> io::Result<NetConn> {
        let reg = Frame::new(kind::REGISTER).num(tag::NAVIGATION, 1).num(tag::PARTITION, partition_of(session));
        let r = self.control(reg)?;
        let token = match (r.kind, r.field(tag::TOKEN)) {
            (kind::REGISTERED, Some(t)) => t.to_vec(),
            _ => return Err(io::Error::other(r.get_str(tag::MESSAGE).unwrap_or("not registered").to_owned())),
        };
        let mut s = UnixStream::connect(&self.socket)?;
        s.set_write_timeout(Some(CONTROL_WAIT))?;
        let mut conn = NetConn { s: s.try_clone()?, buf: Vec::new(), token, next_id: 1 };
        Frame::new(kind::HELLO).num(tag::ID, 0).bytes(tag::TOKEN, &conn.token).write_to(&mut s)?;
        match conn.next(Instant::now() + CONTROL_WAIT)? {
            Some(f) if f.kind == kind::HELLO && f.get_num(tag::OK) == Some(1) => Ok(conn),
            _ => Err(io::Error::other("the network service refused the host's connection")),
        }
    }

    /// Drops a page's registration; its connection is refused after.
    pub fn release(&self, conn: &NetConn) {
        let _ = self.control(Frame::new(kind::UNREGISTER).bytes(tag::TOKEN, &conn.token));
    }

    /// Drops agent session `session`'s cookie partition and its cookies;
    /// whether it was open.
    pub fn end_session(&self, session: u32) -> io::Result<bool> {
        let r = self.control(Frame::new(kind::PARTITION_DROP).num(tag::PARTITION, partition_of(session)))?;
        match r.kind {
            kind::PARTITION_DROP => Ok(r.get_num(tag::OK) == Some(1)),
            _ => Err(io::Error::other(r.get_str(tag::MESSAGE).unwrap_or("not dropped").to_owned())),
        }
    }
}

/// One page's connection to the network service: frames out, and frames
/// in as they arrive, kept whole across reads that time out.
pub struct NetConn {
    s: UnixStream,
    buf: Vec<u8>,
    token: Vec<u8>,
    next_id: u64,
}

impl NetConn {
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
        let mut c = Control::new(host, Duration::from_millis(200)).unwrap();
        let slow = std::thread::spawn(move || {
            let first = Frame::read_from(&mut peer).unwrap().unwrap();
            std::thread::sleep(Duration::from_millis(400));
            let late = Frame::new(kind::REGISTERED).num(tag::ID, first.get_num(tag::ID).unwrap()).bytes(tag::TOKEN, b"session-x-token!");
            late.write_to(&mut peer).unwrap();
            let second = Frame::read_from(&mut peer).unwrap().unwrap();
            let mine = Frame::new(kind::REGISTERED).num(tag::ID, second.get_num(tag::ID).unwrap()).bytes(tag::TOKEN, b"session-y-token!");
            mine.write_to(&mut peer).unwrap();
        });
        let reg = || Frame::new(kind::REGISTER).num(tag::NAVIGATION, 1).num(tag::ID, 1);
        let timed_out = c.request(reg()).unwrap_err();
        assert_eq!(timed_out.kind(), io::ErrorKind::TimedOut);
        // the first reply arrives while the second request waits
        std::thread::sleep(Duration::from_millis(300));
        let r = c.request(reg()).unwrap();
        assert_eq!(r.field(tag::TOKEN), Some(&b"session-y-token!"[..]));
        slow.join().unwrap();
    }

    #[test]
    fn no_session_is_in_the_profiles_partition() {
        assert_eq!(partition_of(0), 1);
        assert_eq!(partition_of(u32::MAX), u64::from(u32::MAX) + 1);
    }
}
