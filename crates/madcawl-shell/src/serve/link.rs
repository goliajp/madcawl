//! The channel between a host and one of its content processes: the
//! agent session's frames, forwarded as they come, and frames of the
//! host's own (`internal`) that no agent may send. While it answers a
//! request, a content process asks its host for what only the host has:
//! a page number, or a registration with the network service, which the
//! host makes in the session's own cookie partition. Its audit lines go
//! to the host too, which writes them under the session's number.

use std::io::{self, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use madcawl_agent::{Frame, FrameError, MAX_FRAME, tag};

/// The kinds only a host and its content process exchange.
pub mod internal {
    /// Host to content: open page `PAGE` at `URL` again, `WIDTH` by
    /// `HEIGHT`, scrolled to `OFFSET`
    pub const RESTORE: u16 = 240;
    /// Host to content: the pages it holds, a line each
    pub const STATE: u16 = 241;
    /// Content to host: register a page's connection with the network
    /// service; the host names the session's partition and answers the
    /// token in `DATA`
    pub const NET_REGISTER: u16 = 242;
    /// Content to host: a number for a page it is opening, or an error
    /// when the host holds as many pages as it may
    pub const NEW_PAGE: u16 = 243;
    /// Host to content: the agent's session has ended; close its pages
    /// and exit
    pub const END: u16 = 244;
    /// Content to host: drop the registration whose token is in `DATA`,
    /// one the host made for this session
    pub const NET_RELEASE: u16 = 245;
    /// Content to host, unanswered: an audit line, its fields (origin,
    /// action, role, name, result) as five `FIELD`s
    pub const AUDIT: u16 = 246;

    /// The tag the host sets on a request naming a page another session
    /// holds, which the content process then refuses as such; the host
    /// strips it from what agents send
    pub const FOREIGN: u8 = 250;

    /// Whether an agent may send `kind`.
    pub fn is_internal(kind: u16) -> bool {
        kind >= 240
    }
}

/// How long a content process waits for its host's answer.
const ASK_WAIT: Duration = Duration::from_secs(30);

/// The next whole frame on `s`, kept in `buf` across reads that time
/// out; `None` once `deadline` passes without one.
pub fn read_until(s: &mut UnixStream, buf: &mut Vec<u8>, deadline: Instant) -> io::Result<Option<Frame>> {
    loop {
        match Frame::decode(buf) {
            Ok((f, used)) => {
                buf.drain(..used);
                return Ok(Some(f));
            }
            Err(FrameError::Short) => {}
            Err(e) => return Err(io::Error::other(format!("a bad frame: {e:?}"))),
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
            Ok(n) if buf.len() + n > MAX_FRAME + 4 => return Err(io::Error::other("a frame too long")),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => return Ok(None),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

pub fn write(s: &mut UnixStream, f: &Frame) -> io::Result<()> {
    let mut out = Vec::new();
    f.encode(&mut out);
    s.write_all(&out)
}

/// A content process's end: questions to the host, asked only while a
/// request is being answered, when the host reads nothing else from it.
pub struct Link(Mutex<(UnixStream, Vec<u8>)>);

impl Link {
    pub fn new(s: UnixStream) -> Link {
        Link(Mutex::new((s, Vec::new())))
    }

    pub fn ask(&self, f: &Frame) -> io::Result<Frame> {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let (s, buf) = &mut *g;
        write(s, f)?;
        let r = read_until(s, buf, Instant::now() + ASK_WAIT)?;
        let _ = s.set_read_timeout(None);
        r.ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "the host did not answer in time"))
    }

    /// The number of a new page, which the host counts against its own
    /// limit; the host's error text when it may not open.
    pub fn new_page(&self) -> io::Result<Result<u32, String>> {
        let r = self.ask(&Frame::new(internal::NEW_PAGE))?;
        Ok(match r.get_num(tag::PAGE) {
            Some(p) => Ok(p as u32),
            None => Err(r.get_str(tag::TEXT).unwrap_or("no page number").to_owned()),
        })
    }

    /// A registration with the network service in the session's cookie
    /// partition, made by the host; its token.
    pub fn net_register(&self) -> io::Result<Vec<u8>> {
        let r = self.ask(&Frame::new(internal::NET_REGISTER))?;
        r.field(tag::DATA).map(<[u8]>::to_vec).ok_or_else(|| io::Error::other(r.get_str(tag::TEXT).unwrap_or("no answer").to_owned()))
    }

    /// Drops a registration `net_register` made.
    pub fn net_release(&self, token: &[u8]) -> io::Result<()> {
        let r = self.ask(&Frame::new(internal::NET_RELEASE).bytes(tag::DATA, token))?;
        match r.kind {
            madcawl_agent::kind::RESULT => Ok(()),
            _ => Err(io::Error::other(r.get_str(tag::TEXT).unwrap_or("not released").to_owned())),
        }
    }

    /// Sends an audit line to the host, which answers nothing.
    pub fn audit(&self, fields: [&str; 5]) -> io::Result<()> {
        let f = fields.iter().fold(Frame::new(internal::AUDIT), |f, v| f.str(tag::FIELD, v));
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        write(&mut g.0, &f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_is_answered_and_a_frame_split_across_reads_is_whole() {
        let (a, mut b) = UnixStream::pair().unwrap();
        let link = Link::new(a);
        let t = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let q = read_until(&mut b, &mut buf, Instant::now() + Duration::from_secs(5)).unwrap().unwrap();
            assert_eq!(q.kind, internal::NEW_PAGE);
            let mut out = Vec::new();
            Frame::new(20).num(tag::PAGE, 7).encode(&mut out);
            b.write_all(&out[..3]).unwrap();
            std::thread::sleep(Duration::from_millis(20));
            b.write_all(&out[3..]).unwrap();
        });
        assert_eq!(link.new_page().unwrap(), Ok(7));
        t.join().unwrap();
        assert!(internal::is_internal(internal::RESTORE) && !internal::is_internal(madcawl_agent::kind::STATS));
    }
}
