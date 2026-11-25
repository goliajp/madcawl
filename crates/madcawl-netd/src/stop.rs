//! Stopping on SIGTERM, SIGINT or SIGHUP with the profile saved: the
//! signals are blocked before any thread starts, so every thread inherits
//! the mask, and one thread takes them with `sigwait` and runs the stop
//! action.

use std::ffi::c_int;

const SIGHUP: c_int = 1;
const SIGINT: c_int = 2;
const SIGTERM: c_int = 15;
#[cfg(target_os = "macos")]
const SIG_BLOCK: c_int = 1;
#[cfg(not(target_os = "macos"))]
const SIG_BLOCK: c_int = 0;

/// room for `sigset_t` on every platform this builds for
#[repr(C, align(8))]
pub struct SigSet([u8; 128]);

unsafe extern "C" {
    fn sigemptyset(set: *mut SigSet) -> c_int;
    fn sigaddset(set: *mut SigSet, sig: c_int) -> c_int;
    fn pthread_sigmask(how: c_int, set: *const SigSet, old: *mut SigSet) -> c_int;
    fn sigwait(set: *const SigSet, sig: *mut c_int) -> c_int;
}

/// Blocks SIGTERM, SIGINT and SIGHUP in the calling thread; call it before any
/// other thread starts.
pub fn block() -> SigSet {
    let mut set = SigSet([0; 128]);
    unsafe {
        sigemptyset(&mut set);
        sigaddset(&mut set, SIGTERM);
        sigaddset(&mut set, SIGINT);
        sigaddset(&mut set, SIGHUP);
        pthread_sigmask(SIG_BLOCK, &set, std::ptr::null_mut());
    }
    set
}

/// Runs `f` on a thread of its own once one of the blocked signals
/// arrives.
pub fn on_signal(set: SigSet, f: impl FnOnce() + Send + 'static) {
    let spawned = std::thread::Builder::new().name("netd-signals".into()).spawn(move || {
        let mut sig = 0;
        loop {
            if unsafe { sigwait(&set, &mut sig) } == 0 {
                break;
            }
        }
        f();
    });
    if let Err(e) = spawned {
        eprintln!("madcawl-netd: signal thread: {e}");
    }
}
