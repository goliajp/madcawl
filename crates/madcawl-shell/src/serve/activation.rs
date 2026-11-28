//! Started by a parent that holds the socket (socket activation): the
//! host accepts on the listening socket it inherited, and leaves once
//! no session has been open for a while. Connections that arrive while
//! it is leaving wait in the socket's queue for the next host the
//! parent starts.

use std::io::{Error, ErrorKind};
use std::mem::ManuallyDrop;
use std::os::fd::FromRawFd as _;
use std::os::unix::fs::FileTypeExt as _;
use std::os::unix::net::UnixListener;
use std::time::Duration;

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[cfg(target_os = "macos")]
type NFds = u32;
#[cfg(not(target_os = "macos"))]
type NFds = u64;

const POLLIN: i16 = 1;
const F_SETFD: i32 = 2;
const FD_CLOEXEC: i32 = 1;
const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;
#[cfg(target_os = "macos")]
const O_NONBLOCK: i32 = 0x4;
#[cfg(not(target_os = "macos"))]
const O_NONBLOCK: i32 = 0o4000;

unsafe extern "C" {
    fn poll(fds: *mut PollFd, n: NFds, timeout_ms: i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

/// The listening socket the parent left at `fd`; anything but a socket
/// is refused here. A socket that does not listen is found at the
/// first accept (see `fatal`): macOS does not answer SO_ACCEPTCONN for
/// Unix sockets.
pub(crate) fn inherited(fd: i32) -> std::io::Result<UnixListener> {
    // SAFETY: borrowed only to read its type; ManuallyDrop keeps it open
    let probe = ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) });
    if !probe.metadata()?.file_type().is_socket() {
        return Err(Error::new(ErrorKind::InvalidInput, format!("fd {fd} is not a socket")));
    }
    // SAFETY: fcntl on a descriptor we checked is open; sessions the
    // host starts never run a program, but nothing it starts gets it
    if unsafe { fcntl(fd, F_SETFD, FD_CLOEXEC) } != 0 {
        return Err(Error::last_os_error());
    }
    // the flags are shared with the parent's copy: a parent that set
    // O_NONBLOCK would make accept spin and, on macOS, hand the flag to
    // every accepted connection
    // SAFETY: as above
    let flags = unsafe { fcntl(fd, F_GETFL) };
    if flags < 0 || (flags & O_NONBLOCK != 0 && unsafe { fcntl(fd, F_SETFL, flags & !O_NONBLOCK) } != 0) {
        return Err(Error::last_os_error());
    }
    // SAFETY: the fd is the parent's gift to this process, owned from here on
    Ok(unsafe { UnixListener::from_raw_fd(fd) })
}

/// Whether `fd` has a connection waiting within `wait`.
pub(crate) fn readable(fd: i32, wait: Duration) -> std::io::Result<bool> {
    let ms = wait.as_millis().clamp(1, i32::MAX as u128) as i32;
    let mut p = PollFd { fd, events: POLLIN, revents: 0 };
    loop {
        // SAFETY: one valid pollfd for the length of the call
        let r = unsafe { poll(&mut p, 1, ms) };
        if r >= 0 {
            return Ok(r > 0);
        }
        let e = Error::last_os_error();
        if e.kind() != ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// Waits until `a` or `b` (when given) has something to read or is
/// closed, or `wait` passes (no limit when `None`); which of them.
pub(crate) fn readable_pair(a: i32, b: Option<i32>, wait: Option<Duration>) -> std::io::Result<(bool, bool)> {
    let ms = wait.map_or(-1, |w| w.as_millis().clamp(1, i32::MAX as u128) as i32);
    const POLLERR: i16 = 0x8;
    const POLLHUP: i16 = 0x10;
    let mut p = [PollFd { fd: a, events: POLLIN, revents: 0 }, PollFd { fd: b.unwrap_or(-1), events: POLLIN, revents: 0 }];
    let n = if b.is_some() { 2 } else { 1 };
    loop {
        // SAFETY: `n` valid pollfds for the length of the call
        let r = unsafe { poll(p.as_mut_ptr(), n, ms) };
        if r >= 0 {
            let ready = |q: &PollFd| q.revents & (POLLIN | POLLERR | POLLHUP) != 0;
            return Ok((ready(&p[0]), n == 2 && ready(&p[1])));
        }
        let e = Error::last_os_error();
        if e.kind() != ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// Waits, with no limit, until one of `fds` has something to read or is
/// closed; which of them, into `ready`.
pub(crate) fn readable_any(fds: &[i32], ready: &mut Vec<bool>) -> std::io::Result<()> {
    const POLLERR: i16 = 0x8;
    const POLLHUP: i16 = 0x10;
    let mut p: Vec<PollFd> = fds.iter().map(|&fd| PollFd { fd, events: POLLIN, revents: 0 }).collect();
    loop {
        // SAFETY: `p.len()` valid pollfds for the length of the call
        let r = unsafe { poll(p.as_mut_ptr(), p.len() as NFds, -1) };
        if r >= 0 {
            ready.clear();
            ready.extend(p.iter().map(|q| q.revents & (POLLIN | POLLERR | POLLHUP) != 0));
            return Ok(());
        }
        let e = Error::last_os_error();
        if e.kind() != ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// An accept error that every later accept would repeat: the inherited
/// socket is not one to accept on.
pub(crate) fn fatal(e: &Error) -> bool {
    // EINVAL not listening, EBADF, ENOTSOCK, ENOTSUP / EOPNOTSUPP
    #[cfg(target_os = "macos")]
    return matches!(e.raw_os_error(), Some(22 | 9 | 38 | 45 | 102));
    #[cfg(not(target_os = "macos"))]
    return matches!(e.raw_os_error(), Some(22 | 9 | 88 | 95));
}
