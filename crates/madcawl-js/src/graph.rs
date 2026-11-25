//! What the cycle collector's callbacks report to, and how far a
//! collection step goes.

use crate::engine::Engine;
use crate::value::{HostRef, Owned};

/// What `traverse` reports to. Each call is one counted edge.
pub trait Tracer<E: Engine> {
    /// An edge to another host object.
    fn host(&mut self, obj: HostRef);
    /// An edge to a script value: a member handle, a preserved wrapper.
    fn value(&mut self, v: E::Value);
}

/// What `unlink` gives its edges up to. The engine releases them when
/// the batch is over, so every object of the cycle stays readable until
/// then.
pub trait Unlinker<E: Engine> {
    fn release(&mut self, v: Owned<E::Value>);
}

#[derive(Clone, Copy, Debug)]
pub enum Deadline {
    /// Stop after this many units of work (an object visited or
    /// unlinked): deterministic, for tests and for budgets.
    Work(u32),
    /// Stop once the host's clock ([`crate::Host::now`]) reads this
    /// many nanoseconds or more. The engine reads no clock of its own;
    /// it asks the host every so many units of work.
    At(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The collection is not finished; call again.
    More,
    /// Nothing more to do until new candidates come.
    Done,
}
