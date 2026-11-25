//! Reading a guarded connection before it has bound itself with HELLO.
//! The client is a stranger until then, so the frame it may send is small
//! and the whole of it must arrive by a deadline: a timeout on each read
//! alone would let a client that sends one byte at a time keep its slot,
//! and the buffer for the length it announced, for good.

use madcawl_net::wire::Frame;
use std::io::{self, Read};
use std::os::unix::net::UnixStream;
use std::time::Instant;

/// far more than a HELLO with its token takes
const MAX_HELLO_FRAME: usize = 4 << 10;

/// A stream whose reads all end by the deadline it carries.
struct Until<'a>(&'a UnixStream, Instant);

impl Read for Until<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.1.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.0.set_read_timeout(Some(left))?;
        let mut s = self.0;
        s.read(buf)
    }
}

/// The next frame on `r`; while `unbound`, at most `MAX_HELLO_FRAME`
/// bytes of it and all of them before `deadline`.
pub(crate) fn read_next(r: &mut UnixStream, unbound: bool, deadline: Instant) -> io::Result<Option<Frame>> {
    match unbound {
        true => Frame::read_capped(&mut Until(r, deadline), MAX_HELLO_FRAME),
        false => Frame::read_from(r),
    }
}
