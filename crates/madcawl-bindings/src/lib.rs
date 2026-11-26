//! DOM bindings: the interfaces of `idl/` made script objects, generic
//! over the engine interface, so the same code is compiled for each
//! engine.
//!
//! A node's wrapper is made the first time script reads the node and
//! kept in the node's wrapper word. Listeners are member handles in a
//! side table by node index; on an engine that keeps wrappers by reach
//! they live beside the node's wrapper and go with it. The counts on
//! nodes follow the DOM's rules; [`DomHost::settle`] passes on what
//! changed (wrappers gone, nodes freed, nodes left holding cycles), and
//! [`DomHost::collect`] runs the engine's cycle collector by deadline.

mod abort;
mod args;
mod attr;
mod convert;
mod event;
mod event_loop;
mod fixed;
mod gc;
mod holds;
mod host;
mod impls;
mod listeners;
mod lists;
mod parse;
mod shell;
mod side;
mod side_ops;
mod trees;
mod window;

#[rustfmt::skip]
mod generated;

pub use convert::Thrown;
pub use event_loop::{Clock, EventLoop};
pub use gc::NodeOps;
pub use generated::{DOMExceptionClass, DocumentClass, ElementClass, EventClass, EventTargetClass, NodeClass};
pub use side::SideRef;
pub use side_ops::SideOps;
pub use host::DomHost;
pub use parse::Loader;
