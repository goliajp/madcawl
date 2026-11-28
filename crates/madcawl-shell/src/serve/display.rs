//! A connection playing the display end: the program that shows pane
//! pages (marspot's core). Its first frame is `HELLO` with the display
//! role; once the version ranges meet, each `PANE_OPEN` opens a page in
//! a content process of its own (`proxy::Pane`) and the answer,
//! `PANE_OPENED`, carries the display channel to that process. Pane
//! pages do not close for being idle, and while a display end is
//! connected the host does not count as idle either: the connection is
//! a session, and an open session keeps the host up.
//!
//! Who may play the display end is not checked beyond the socket's own
//! permission (only the user may connect).

use std::os::fd::{AsFd as _, AsRawFd as _};
use std::os::unix::net::UnixStream;
use std::io::ErrorKind;
use std::time::Duration;

use madcawl_agent::Frame;
use madcawl_wire::host::{self, PaneOpen, PaneOpened, role};
use madcawl_wire::{Hello, Outcome, os};

use super::log::Line;
use super::proxy::Pane;
use super::wire::{BODY_WAIT, Got, read_frame};
use super::{Shared, activation, code, error_frame};

const MSG_PEEK: i32 = 2;
#[cfg(target_os = "macos")]
const MSG_WAITALL: i32 = 0x40;
#[cfg(not(target_os = "macos"))]
const MSG_WAITALL: i32 = 0x100;

/// A frame's length and kind, all that routing reads.
const FRAME_HEAD: usize = 6;

unsafe extern "C" {
    fn recv(fd: i32, buf: *mut u8, len: usize, flags: i32) -> isize;
}

/// What a new connection is, from its first bytes.
pub(crate) enum Route {
    Display,
    Agent,
    /// Nothing came for the idle time
    Idle,
}

/// Looks at a new connection's first frame head without taking it: a
/// frame of the host's `HELLO` kind makes it the display end's. Agent
/// kinds are all below it, and the hello's fields, however many a newer
/// display end sends, are read by [`run`] from the whole frame. A head
/// that comes in pieces is waited for as a frame's rest is, unless its
/// length already makes it an error the agent path answers.
pub(crate) fn route(s: &UnixStream, idle: Duration, max: usize) -> Route {
    let mut head = [0u8; FRAME_HEAD];
    let n = match peek(s, &mut head, idle, 0) {
        Ok(n) => n,
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return Route::Idle,
        Err(_) => return Route::Agent,
    };
    let len = (n >= 4).then(|| u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize);
    let n = match n {
        FRAME_HEAD => n,
        0 => return Route::Agent,
        _ if len.is_some_and(|l| !(2..=max).contains(&l)) => return Route::Agent,
        _ => peek(s, &mut head, BODY_WAIT, MSG_WAITALL).unwrap_or(0),
    };
    match n == FRAME_HEAD && u16::from_le_bytes([head[4], head[5]]) == host::kind::HELLO {
        true => Route::Display,
        false => Route::Agent,
    }
}

/// Peeks at up to `head.len()` bytes, waiting `wait` at most.
fn peek(s: &UnixStream, head: &mut [u8], wait: Duration, flags: i32) -> std::io::Result<usize> {
    s.set_read_timeout(Some(wait))?;
    loop {
        // SAFETY: `head` is writable for its length; MSG_PEEK leaves the bytes queued
        let n = unsafe { recv(s.as_raw_fd(), head.as_mut_ptr(), head.len(), MSG_PEEK | flags) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = std::io::Error::last_os_error();
        if e.kind() != ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

fn log(shared: &Shared, id: u32, action: &str, result: &str) {
    shared.log.write(&Line { session: id, origin: "-", action, role: "", name: "", result });
}

fn send(s: &mut UnixStream, f: &Frame) -> bool {
    let mut out = Vec::new();
    f.encode(&mut out);
    std::io::Write::write_all(s, &out).is_ok()
}

/// Serves display end `id` on `s` until it hangs up; its panes close
/// with it.
pub(crate) fn run(id: u32, shared: &Shared, mut s: UnixStream) {
    let max = shared.config.max_request;
    let first = match read_frame(&mut s, max, None) {
        Got::Frame(f) => Hello::from_frame(&f).ok().filter(|h| h.role == Some(role::DISPLAY)),
        Got::TooLong(_) | Got::Bad => None,
        Got::Idle | Got::Closed => return,
    };
    let Some(theirs) = first else {
        log(shared, id, "display-hello", "error 10");
        send(&mut s, &error_frame(code::BAD_REQUEST, "a host hello without the display role or with bad fields"));
        return;
    };
    let (answer, outcome) = Hello::answer(host::RANGE, &theirs);
    let sent = send(&mut s, &answer.frame(host::kind::HELLO));
    let Outcome::Agreed(v) = outcome else {
        log(shared, id, "display-hello", &format!("refused {outcome:?}"));
        return;
    };
    log(shared, id, "display-hello", &format!("version {v}"));
    if !sent {
        return;
    }
    let mut panes: Vec<Pane> = Vec::new();
    serve(id, shared, s, &mut panes);
    for p in panes {
        p.end();
    }
    log(shared, id, "display-end", "ok");
}

fn serve<'a>(id: u32, shared: &'a Shared, mut s: UnixStream, panes: &mut Vec<Pane<'a>>) {
    let max = shared.config.max_request;
    let (mut fds, mut ready) = (Vec::new(), Vec::new());
    loop {
        fds.clear();
        fds.push(s.as_raw_fd());
        fds.extend(panes.iter().map(|p| p.fd().unwrap_or(-1)));
        if activation::readable_any(&fds, &mut ready).is_err() {
            return;
        }
        // panes whose content process ended go first, before a request
        let mut i = panes.len();
        while i > 0 {
            i -= 1;
            if ready[i + 1] && !panes[i].spoke() {
                panes.remove(i).end();
            }
        }
        if !ready[0] {
            continue;
        }
        let f = match read_frame(&mut s, max, None) {
            Got::Frame(f) => f,
            Got::Idle => continue,
            Got::Closed => return,
            Got::TooLong(_) | Got::Bad => {
                send(&mut s, &error_frame(code::BAD_REQUEST, "a malformed or oversized frame"));
                return;
            }
        };
        let ok = match f.kind {
            host::kind::PANE_OPEN => open(id, shared, &mut s, &f, panes),
            k => {
                log(shared, id, &format!("display-kind-{k}"), "error 6");
                send(&mut s, &error_frame(code::UNKNOWN, "unknown request kind"))
            }
        };
        if !ok {
            return;
        }
    }
}

/// Opens a pane and answers; false once the display end cannot be
/// written to.
fn open<'a>(id: u32, shared: &'a Shared, s: &mut UnixStream, f: &Frame, panes: &mut Vec<Pane<'a>>) -> bool {
    let o = match PaneOpen::from_frame(f) {
        Ok(o) => o,
        Err(e) => return send(s, &error_frame(code::BAD_REQUEST, &format!("a bad pane request: {e}"))),
    };
    if panes.len() >= shared.config.max_pages {
        let msg = format!("this display end has {} panes open already", panes.len());
        return send(s, &error_frame(code::TOO_MANY_PAGES, &msg));
    }
    let pane_id = shared.next_session.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let (pane, theirs) = match Pane::open(pane_id, shared, &o) {
        Ok(p) => p,
        Err(r) => {
            log(shared, id, "pane-open", &format!("error {}", r.get_num(madcawl_agent::tag::CODE).unwrap_or(0)));
            return send(s, &r);
        }
    };
    let opened = PaneOpened { session: o.session, page: pane.page, pid: pane.pid() };
    let mut out = Vec::new();
    opened.frame().encode(&mut out);
    panes.push(pane);
    // the display end's side of the channel goes with the answer, and
    // this process keeps no copy of it
    os::send(s, &out, &[theirs.as_fd()]).is_ok()
}
