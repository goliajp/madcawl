//! The host's end of the network service's control channel, and reading
//! and writing the service's frames with ids and deadlines.

use std::io::{self, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use madcawl_net::wire::{Frame, FrameError, MAX_FRAME, tag};

/// The host's end of the control channel. Each request has an id of its
/// own and only the reply with that id answers it: a reply that comes
/// after its request gave up would otherwise answer the next request,
/// handing one session another's registration.
pub(super) struct Control {
    s: UnixStream,
    buf: Vec<u8>,
    next_id: u64,
    wait: Duration,
    /// Which network process the channel is to, counting from 0
    pub generation: u64,
    pub braked: bool,
}

impl Control {
    pub fn new(s: UnixStream, wait: Duration) -> io::Result<Control> {
        s.set_write_timeout(Some(wait))?;
        Ok(Control { s, buf: Vec::new(), next_id: 0, wait, generation: 0, braked: false })
    }

    pub fn request(&mut self, f: Frame) -> io::Result<Frame> {
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
    use madcawl_net::wire::kind;

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
        });
        let reg = || Frame::new(kind::REGISTER).num(tag::NAVIGATION, 1).num(tag::ID, 1);
        let timed_out = c.request(reg()).unwrap_err();
        assert_eq!(timed_out.kind(), io::ErrorKind::TimedOut);
        timed_out_tx.send(()).unwrap();
        // the late reply is on the stream ahead of the second one
        let r = c.request(reg()).unwrap();
        assert_eq!(r.field(tag::TOKEN), Some(&b"session-y-token!"[..]));
        slow.join().unwrap();
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
