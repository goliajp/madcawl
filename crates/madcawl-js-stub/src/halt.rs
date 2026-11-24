//! Halting script on an interrupt request.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use madcawl_js::{Control, Halt, Interrupt, Unsupported};

use crate::cell::Cell;
use crate::isolate::Stub;
use crate::value::Value;

pub(crate) const TERMINATE: u8 = 1;
pub(crate) const ABORT: u8 = 4;

impl Stub {
    /// Take a pending interrupt request: a termination drops the queued
    /// microtasks, and wins over an abort; an abort seen while no script
    /// runs is dropped.
    pub(crate) fn poll_interrupt(&mut self) {
        let bits = self.interrupt.fetch_and(!(TERMINATE | ABORT), Ordering::Relaxed);
        if bits & TERMINATE != 0 && self.halt != Halt::Terminated {
            self.halt = Halt::Terminated;
            self.drop_microtasks();
        } else if bits & ABORT != 0 && self.halt == Halt::Running && self.depth != 0 {
            self.halt = Halt::Aborted;
        }
    }

    pub(crate) fn halted(&self) -> bool {
        self.halt != Halt::Running
    }

    pub(crate) fn drop_microtasks(&mut self) {
        while let Some((j, a)) = self.microtasks.pop_front() {
            self.release_value(j);
            self.release_value(a);
        }
    }

    pub(crate) fn queue_microtask(&mut self, job: Value, arg: Value) {
        self.poll_interrupt();
        if self.halt == Halt::Terminated {
            self.release_value(job);
            self.release_value(arg);
        } else {
            self.microtasks.push_back((job, arg));
        }
    }

    pub(crate) fn resume_script(&mut self) {
        assert_eq!(self.depth, 0, "resumed from inside a call");
        self.poll_interrupt();
        assert_ne!(self.halt, Halt::Terminated, "a terminated isolate resumed");
        self.halt = Halt::Running;
    }
}

#[derive(Clone)]
pub struct StubInterrupt(Arc<AtomicU8>);

impl Interrupt for StubInterrupt {
    fn terminate(&self) {
        self.0.fetch_or(TERMINATE, Ordering::Relaxed);
    }

    fn abort(&self) -> Result<(), Unsupported> {
        self.0.fetch_or(ABORT, Ordering::Relaxed);
        Ok(())
    }

    fn request_callback(&self) {
        self.0.fetch_or(2, Ordering::Relaxed);
    }
}

impl Control for Stub {
    type Interrupt = StubInterrupt;

    fn interrupt_handle(&self) -> StubInterrupt {
        StubInterrupt(self.interrupt.clone())
    }

    fn set_memory_limits(&mut self, soft: usize, hard: usize) {
        self.limits = (soft, hard);
    }

    fn adjust_external_memory(&mut self, delta: isize) {
        self.external += delta;
    }

    fn memory_usage(&self) -> usize {
        self.live * size_of::<Cell>() + self.external.max(0) as usize
    }
}
