//! Reading one request frame off a session's socket, bounded in size
//! and in how long its bytes may take once they start.

use std::io::{ErrorKind, Read as _};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use madcawl_agent::{Frame, kind};

/// How long the rest of a frame may take once its first byte came.
const BODY_WAIT: Duration = Duration::from_secs(10);

pub(crate) enum Got {
    Frame(Frame),
    /// Nothing came before the earliest page's idle time ran out
    Idle,
    Closed,
    TooLong(usize),
    Bad,
}

pub(crate) fn read_frame(s: &mut UnixStream, max: usize, wait: Option<Duration>) -> Got {
    let mut head = [0u8; 4];
    if s.set_read_timeout(wait).is_err() {
        return Got::Closed;
    }
    loop {
        match s.read(&mut head[..1]) {
            Ok(0) => return Got::Closed,
            Ok(_) => break,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Got::Idle;
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Got::Closed,
        }
    }
    if s.set_read_timeout(Some(BODY_WAIT)).is_err() || s.read_exact(&mut head[1..]).is_err() {
        return Got::Closed;
    }
    let len = u32::from_le_bytes(head) as usize;
    if len > max {
        return Got::TooLong(len);
    }
    let mut buf = vec![0u8; 4 + len];
    buf[..4].copy_from_slice(&head);
    if s.read_exact(&mut buf[4..]).is_err() {
        return Got::Closed;
    }
    match Frame::decode(&buf) {
        Ok((f, _)) => Got::Frame(f),
        Err(_) => Got::Bad,
    }
}

/// A request kind's name, for the watchdog's lines.
pub(crate) fn kind_name(k: u16) -> &'static str {
    match k {
        kind::OPEN => "open",
        kind::CLOSE => "close",
        kind::PAGES => "pages",
        kind::READ => "read",
        kind::DIFF => "diff",
        kind::ACT => "act",
        kind::WAIT => "wait",
        kind::REQUESTS => "requests",
        kind::SCREENSHOT => "screenshot",
        kind::STATS => "stats",
        _ => "other",
    }
}
