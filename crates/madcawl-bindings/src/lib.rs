//! DOM bindings: the interfaces of `idl/` made script objects, generic
//! over the engine interface, so the same code is compiled for each
//! engine.
//!
//! A node's wrapper is made the first time script reads the node and
//! kept in the node's wrapper word. Listeners are member handles in a
//! side table by node index, so a node without a wrapper keeps its
//! listeners and stays a vertex of the collector's graph. The counts on
//! nodes follow the DOM's rules; [`DomHost::settle`] passes on what
//! changed (wrappers gone, nodes freed, nodes left holding cycles), and
//! [`DomHost::collect`] runs the engine's cycle collector by deadline.

mod convert;
mod event_loop;
mod gc;
mod host;
mod impls;
mod parse;
mod shell;
mod trees;
mod window;

#[rustfmt::skip]
mod generated;

pub use convert::Thrown;
pub use event_loop::{Clock, EventLoop};
pub use gc::NodeOps;
pub use generated::{DocumentClass, ElementClass, EventTargetClass, NodeClass};
pub use host::DomHost;
pub use parse::Loader;
