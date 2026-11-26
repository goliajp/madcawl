//! Stopping script from another thread, and the memory limits.
//!
//! A request sets a bit; the watchdog (`private.rs`) reads the bits
//! while script runs and ends it, and every entry into the isolate
//! reads them before running anything. The engine's end of a script is
//! not final, so an abort is recoverable: the isolate stays usable
//! after `resume`, its microtask queue kept.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use madcawl_js::{Control, Halt, Host, Interrupt, Unsupported};

use crate::isolate::Jsc;

pub(crate) const TERMINATE: u8 = 1;
pub(crate) const ABORT: u8 = 2;
pub(crate) const CALLBACK: u8 = 4;

#[derive(Default)]
pub struct Flags {
    pub(crate) bits: AtomicU8,
    /// The context group the watchdog is installed on, for re-arming it.
    pub(crate) group: AtomicUsize,
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
}

impl Jsc {
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
            self.flags.bits.fetch_and(!ABORT, Ordering::AcqRel);
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
            }
            if self.depth == 0 {
                self.flags.bits.fetch_and(!ABORT, Ordering::AcqRel);
            }
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
