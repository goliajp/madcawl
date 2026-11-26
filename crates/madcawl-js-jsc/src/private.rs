//! The framework entry points the adapter uses that the system exports
//! but the SDK headers do not declare, looked up by name. Each is one
//! of the list the architecture allows; [`check`] says which are
//! missing, so that a content process refuses to start instead of
//! failing later.
//!
//! - `JSContextGroupSetExecutionTimeLimit`: the watchdog that stops
//!   running script when another thread asks ([`install_watchdog`]).

use std::ffi::{CStr, c_void};
use std::sync::atomic::Ordering;

use crate::ffi::{JSContextGroupRef, JSContextRef};
use crate::halt::{ABORT, Flags, TERMINATE};

type ShouldTerminate = unsafe extern "C" fn(JSContextRef, *mut c_void) -> bool;
type SetTimeLimit = unsafe extern "C" fn(JSContextGroupRef, f64, Option<ShouldTerminate>, *mut c_void);

const SET_TIME_LIMIT: &CStr = c"JSContextGroupSetExecutionTimeLimit";

/// How often running script looks at the interrupt flags, in seconds:
/// the bound on how long a stop request waits.
const WATCH_INTERVAL: f64 = 0.01;

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
}

const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

fn lookup(name: &CStr) -> *mut c_void {
    // SAFETY: a lookup by name in the images loaded
    unsafe { dlsym(RTLD_DEFAULT, name.as_ptr()) }
}

/// The names of the entry points the adapter needs and the system does
/// not export; empty when all are there.
pub fn check() -> Vec<&'static str> {
    [SET_TIME_LIMIT]
        .into_iter()
        .filter(|n| lookup(n).is_null())
        .map(|n| n.to_str().expect("an ASCII name"))
        .collect()
}

// the framework arms its timer once per entry and asks once: armed
// again either way, so that script a stop did not end (script that
// caught the exception a host function threw while halted, the script
// a nested call returned to) is asked again
unsafe extern "C" fn should_terminate(_: JSContextRef, data: *mut c_void) -> bool {
    // SAFETY: the flags the isolate keeps alive while its group lives
    let flags = unsafe { &*data.cast::<Flags>() };
    arm(flags.group.load(Ordering::Relaxed) as JSContextGroupRef, data.cast());
    flags.bits.load(Ordering::Acquire) & (TERMINATE | ABORT) != 0
}

fn arm(group: JSContextGroupRef, flags: *const Flags) {
    let f = lookup(SET_TIME_LIMIT);
    assert!(!f.is_null(), "the system JavaScriptCore exports {SET_TIME_LIMIT:?}");
    // SAFETY: the exported function of this signature; the flags outlive
    // the group (the isolate drops the group first)
    unsafe {
        let set: SetTimeLimit = std::mem::transmute::<*mut c_void, SetTimeLimit>(f);
        set(group, WATCH_INTERVAL, Some(should_terminate), flags.cast_mut().cast());
    }
}

/// Have script that runs in `group` stop within the interval once a
/// stop is asked for in `flags`.
pub(crate) fn install_watchdog(group: JSContextGroupRef, flags: *const Flags) {
    // SAFETY: the isolate's flags, alive
    unsafe { &*flags }.group.store(group as usize, Ordering::Relaxed);
    arm(group, flags);
}

const SYNC_GC: &CStr = c"JSSynchronousGarbageCollectForDebugging";
type SyncGc = unsafe extern "C" fn(JSContextRef);

impl crate::isolate::Jsc {
    /// A full collection and sweep now, finalizers run: for tests and
    /// gauges, which need one at a known moment. Not on the path of a
    /// page, whose collections the framework times itself; so the entry
    /// point is looked up here and is not among those [`check`] needs.
    pub fn collect_now(&mut self) {
        let f = lookup(SYNC_GC);
        assert!(!f.is_null(), "the system JavaScriptCore exports {SYNC_GC:?}");
        // SAFETY: the exported function of this signature, on a live context
        unsafe { std::mem::transmute::<*mut c_void, SyncGc>(f)(self.base) };
    }
}
