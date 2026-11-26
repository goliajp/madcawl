//! A process the host starts ends with the host: a thread blocked in
//! `kevent` on the parent's exit, which costs no wakeups until then. It
//! keeps none of the host's descriptors but its standard three.

use std::io;

#[repr(C)]
struct Kevent {
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: isize,
    udata: *mut core::ffi::c_void,
}

const EVFILT_PROC: i16 = -5;
const EV_ADD: u16 = 0x1;
const EV_ONESHOT: u16 = 0x10;
const NOTE_EXIT: u32 = 0x8000_0000;

unsafe extern "C" {
    fn kqueue() -> i32;
    fn kevent(kq: i32, changes: *const Kevent, nchanges: i32, events: *mut Kevent, nevents: i32, timeout: *const core::ffi::c_void) -> i32;
    fn getppid() -> i32;
    fn getpid() -> i32;
    fn close(fd: i32) -> i32;
}

/// Blocks until process `pid` exits; an error when it cannot be watched
/// (it is gone already, for one).
fn wait_exit(pid: i32) -> io::Result<()> {
    // SAFETY: kqueue takes no arguments
    let kq = unsafe { kqueue() };
    if kq < 0 {
        return Err(io::Error::last_os_error());
    }
    let change = Kevent { ident: pid as usize, filter: EVFILT_PROC, flags: EV_ADD | EV_ONESHOT, fflags: NOTE_EXIT, data: 0, udata: std::ptr::null_mut() };
    let mut got = Kevent { ident: 0, filter: 0, flags: 0, fflags: 0, data: 0, udata: std::ptr::null_mut() };
    loop {
        // SAFETY: one change and room for one event, both live for the call; no timeout
        let n = unsafe { kevent(kq, &change, 1, &mut got, 1, std::ptr::null()) };
        let e = io::Error::last_os_error();
        if n < 0 && e.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        // SAFETY: the queue is this function's own
        unsafe { close(kq) };
        return if n < 0 { Err(e) } else { Ok(()) };
    }
}

/// Closes every descriptor above standard error, which a child of the
/// host inherited without asking for it (a listening socket handed to
/// the host, say); called first thing, before any thread starts.
pub(crate) fn close_inherited_fds() {
    // SAFETY: getpid has no preconditions
    let fds = match crate::rusage::fds(unsafe { getpid() }) {
        Ok(f) => f,
        Err(e) => return eprintln!("madcawl-shell: cannot list the inherited descriptors: {e}"),
    };
    for fd in fds.into_iter().filter(|&fd| fd > 2) {
        // SAFETY: nothing in this process holds these yet
        unsafe { close(fd) };
    }
}

/// Exits this process when its parent does.
pub(crate) fn exit_with_parent() {
    // SAFETY: getppid has no preconditions
    let parent = unsafe { getppid() };
    if parent == 1 {
        std::process::exit(0);
    }
    let spawned = std::thread::Builder::new().name("parent-watch".into()).spawn(move || {
        // either way the parent is gone, or cannot be watched
        let _ = wait_exit(parent);
        std::process::exit(0);
    });
    if let Err(e) = spawned {
        eprintln!("madcawl-shell: cannot watch the host process: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_exit_is_seen() {
        let mut c = std::process::Command::new("/bin/sleep").arg("0.2").spawn().unwrap();
        let pid = c.id() as i32;
        let t = std::time::Instant::now();
        wait_exit(pid).unwrap();
        assert!(t.elapsed() >= std::time::Duration::from_millis(150));
        c.wait().unwrap();
    }
}
