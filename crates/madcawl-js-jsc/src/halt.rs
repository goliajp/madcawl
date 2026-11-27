//! Stopping script from another thread, and the memory and time limits.
//!
//! A request sets a bit; the watchdog (`private.rs`) reads the bits
//! while script runs and ends it, and every entry into the isolate
//! reads them before running anything. The time limit is the watchdog
//! asking for an abort itself once script has run that long. The
//! engine's end of a script is not final, so an abort is recoverable:
//! the isolate stays usable after `resume`, with the microtasks queued
//! before it kept, unless the stop met a promise job the framework ran
//! as a call returned: the framework drops the jobs queued after that
//! one ([`AbortCause::TimeLimitInJob`]).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use madcawl_js::{AbortCause, Control, Halt, Host, Interrupt, Unsupported};

use crate::isolate::Jsc;

pub(crate) const TERMINATE: u8 = 1;
pub(crate) const ABORT: u8 = 2;
pub(crate) const CALLBACK: u8 = 4;
/// With `ABORT`: the abort is the time limit's.
pub(crate) const TIMED: u8 = 8;

#[derive(Default)]
pub struct Flags {
    pub(crate) bits: AtomicU8,
    /// The context group the watchdog is installed on, for re-arming it.
    pub(crate) group: AtomicUsize,
    /// The watchdog's intervals script may run for; 0, no limit.
    pub(crate) limit: AtomicU32,
    /// The intervals script has run for since the isolate was entered.
    pub(crate) ticks: AtomicU32,
    /// The watchdog ended script since the isolate was entered.
    pub(crate) ended: AtomicBool,
}

#[derive(Clone)]
pub struct JscInterrupt(Arc<Flags>);

impl Interrupt for JscInterrupt {
    fn terminate(&self) {
        self.0.bits.fetch_or(TERMINATE, Ordering::AcqRel);
    }

    fn abort(&self) -> Result<(), Unsupported> {
        self.0.bits.fetch_or(ABORT, Ordering::AcqRel);
        Ok(())
    }

    fn request_callback(&self) {
        self.0.bits.fetch_or(CALLBACK, Ordering::AcqRel);
    }
}

impl Control for Jsc {
    type Interrupt = JscInterrupt;

    fn interrupt_handle(&self) -> JscInterrupt {
        JscInterrupt(self.flags.clone())
    }

    fn set_memory_limits(&mut self, soft: usize, hard: usize) {
        self.limits = (soft, hard);
    }

    fn adjust_external_memory(&mut self, delta: isize) {
        self.external += delta;
        if self.memory_usage() < self.limits.0 {
            self.pressure_told = false;
        }
    }

    fn memory_usage(&self) -> usize {
        self.external.max(0) as usize
    }

    fn set_time_limit(&mut self, limit: Option<Duration>) -> Result<(), Unsupported> {
        if self.flags.group.load(Ordering::Relaxed) == 0 {
            return Err(Unsupported);
        }
        let intervals = |l: Duration| l.as_secs_f64() / crate::private::WATCH_INTERVAL;
        let ticks = limit.map_or(0, |l| intervals(l).ceil().clamp(1.0, f64::from(u32::MAX)) as u32);
        self.flags.limit.store(ticks, Ordering::Relaxed);
        Ok(())
    }

    fn abort_cause(&self) -> AbortCause {
        self.cause
    }
}

impl Jsc {
    /// Count a call into the framework that can run script; the time
    /// limit counts from the outermost one.
    pub(crate) fn deeper(&mut self) {
        if self.depth == 0 {
            self.flags.ticks.store(0, Ordering::Relaxed);
            self.flags.ended.store(false, Ordering::Relaxed);
        }
        self.depth += 1;
    }

    /// What an entry does first: see the requests sent since the last
    /// one. A termination is final; an abort sent while no script runs
    /// is dropped; a callback request is delivered here and only here,
    /// so one made while script runs waits for the next entry (a call
    /// into the isolate, a host function calling back in): the watchdog
    /// does not call the host from inside the framework's interrupt
    /// check.
    pub(crate) fn poll<H: Host<Jsc>>(&mut self, host: &mut H) {
        let bits = self.flags.bits.load(Ordering::Acquire);
        if bits == 0 {
            self.memory_check(host);
            return;
        }
        if bits & TERMINATE != 0 {
            self.terminated();
        }
        if bits & ABORT != 0 && self.depth == 0 {
            self.flags.bits.fetch_and(!(ABORT | TIMED), Ordering::AcqRel);
        }
        if bits & CALLBACK != 0 {
            self.flags.bits.fetch_and(!CALLBACK, Ordering::AcqRel);
            host.interrupt_callback(self);
        }
        self.memory_check(host);
    }

    /// After a call into the framework came back with an exception: was
    /// it the watchdog ending script? An abort's request stays until no
    /// script is on the stack, so the watchdog ends the script that made
    /// the nested call too.
    pub(crate) fn after_stop(&mut self) {
        let bits = self.flags.bits.load(Ordering::Acquire);
        if bits & TERMINATE != 0 {
            self.terminated();
        } else if bits & ABORT != 0 {
            if self.halt == Halt::Running {
                self.halt = Halt::Aborted;
                self.cause = if bits & TIMED != 0 { AbortCause::TimeLimit } else { AbortCause::Asked };
            }
            if self.depth == 0 {
                self.flags.bits.fetch_and(!(ABORT | TIMED), Ordering::AcqRel);
            }
        }
    }

    /// After a call into the framework came back with no exception:
    /// the framework runs its promise jobs as the outermost call returns
    /// and keeps what ended one to itself, so a stop seen there shows
    /// only in what the watchdog did during this entry (a request that
    /// came after the call returned is the next entry's to drop), and the
    /// framework's own stop stays pending: the next entry would end at
    /// once. An entry that runs nothing takes it.
    pub(crate) fn after_drain(&mut self) {
        if self.depth != 0 || !self.flags.ended.load(Ordering::Relaxed) {
            return;
        }
        // a stop a nested call already took ended script, not a job
        let running = self.halt == Halt::Running;
        self.after_stop();
        if running && self.halt == Halt::Aborted && self.cause == AbortCause::TimeLimit {
            self.cause = AbortCause::TimeLimitInJob;
        }
        let mut exc: crate::ffi::JSValueRef = std::ptr::null();
        // SAFETY: the isolate's own context and a string released here
        unsafe {
            let empty = crate::ffi::JSStringCreateWithUTF8CString(c"".as_ptr());
            let (this, url) = (std::ptr::null_mut(), std::ptr::null_mut());
            crate::ffi::JSEvaluateScript(self.base, empty, this, url, 1, &mut exc);
            crate::ffi::JSStringRelease(empty);
        }
    }

    fn terminated(&mut self) {
        self.halt = Halt::Terminated;
        self.drop_pending();
    }

    fn memory_check<H: Host<Jsc>>(&mut self, host: &mut H) {
        if !self.pressure_told && self.memory_usage() >= self.limits.0 {
            self.pressure_told = true;
            host.memory_pressure(self);
        }
    }
}
