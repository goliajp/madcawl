//! Stopping script from another thread, and the memory limits.

/// What the host may do from any thread, and the memory limits.
pub trait Control {
    type Interrupt: Interrupt;
    fn interrupt_handle(&self) -> Self::Interrupt;
    /// Past `soft` the engine calls back the host; past `hard` it ends
    /// the running script uncatchably.
    fn set_memory_limits(&mut self, soft: usize, hard: usize);
    /// Memory host objects keep alive (nodes, attributes, decoded
    /// images), counted against the limits.
    fn adjust_external_memory(&mut self, delta: isize);
    fn memory_usage(&self) -> usize;
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
    /// resumes it. A request seen while no script runs is dropped, so it
    /// never ends the next task. Optional for an engine; nothing before
    /// J2 depends on it.
    fn abort(&self) -> Result<(), Unsupported>;
    fn request_callback(&self);
}
