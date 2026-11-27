//! Stopping script from another thread, and the memory and time limits.

use std::time::Duration;

/// What the host may do from any thread, and the memory limits.
pub trait Control {
    type Interrupt: Interrupt;
    fn interrupt_handle(&self) -> Self::Interrupt;
    /// Past `soft` the engine calls [`crate::Host::memory_pressure`];
    /// past `hard` it ends the running script uncatchably.
    fn set_memory_limits(&mut self, soft: usize, hard: usize);
    /// Memory host objects keep alive (nodes, attributes, decoded
    /// images), counted against the limits.
    fn adjust_external_memory(&mut self, delta: isize);
    fn memory_usage(&self) -> usize;
    /// Abort script that has run for `limit` since the isolate was last
    /// entered with no script on the stack, as [`Interrupt::abort`] does;
    /// `None` lifts the limit. Time script spends blocked in a host
    /// function may not count. As with any abort, the script that called
    /// in runs on until the engine's next interrupt check sees the stop.
    /// Optional for an engine.
    fn set_time_limit(&mut self, limit: Option<Duration>) -> Result<(), Unsupported>;
    /// Why script is [`Halt::Aborted`]; meaningful only then.
    fn abort_cause(&self) -> AbortCause;
}

/// What aborted script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbortCause {
    /// [`Interrupt::abort`].
    Asked,
    /// The time limit, met in script the host's call ran.
    TimeLimit,
    /// The time limit, met in a promise job the engine ran as the host's
    /// call returned: the jobs queued after it were dropped unrun.
    TimeLimitInJob,
}

/// Where script stands after an [`Interrupt`] request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Halt {
    Running,
    /// [`Interrupt::abort`]: stopped until [`crate::Engine::resume`],
    /// with the microtask queue kept.
    Aborted,
    /// [`Interrupt::terminate`]: no script ever runs in this isolate
    /// again, and its microtasks are dropped unrun, those queued later
    /// included.
    Terminated,
}

/// The engine does not provide the recoverable abort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unsupported;

/// Sent to another thread to stop or call back running script. The
/// engine answers within its interrupt bound in script and its own
/// builtins; host functions are the host's to keep short.
pub trait Interrupt: Send + Sync + Clone {
    /// End the agent: a worker terminated, a page torn down. Final.
    fn terminate(&self);
    /// Stop the script running when the request is seen (a script that
    /// runs too long) and keep the isolate for later tasks once the host
    /// resumes it. A request seen while no script runs is dropped. One
    /// sent between two tasks may still be seen early in the next, so a
    /// watchdog that aborts a task may end the one after it. Optional
    /// for an engine; nothing before J2 depends on it.
    fn abort(&self) -> Result<(), Unsupported>;
    /// Have the isolate call [`crate::Host::interrupt_callback`] on its
    /// own thread, the next time it is entered or reaches a point where
    /// it checks for interrupts.
    fn request_callback(&self);
}
