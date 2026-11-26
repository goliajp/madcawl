//! The engine interface on the system JavaScriptCore.framework, through
//! its C interface: the transition engine until torajs-engine replaces
//! it, after which this crate is deleted. No other crate names the
//! framework or any of its symbols.
//!
//! How the interface's reference-counting model is kept on a tracing
//! engine:
//!
//! - A value is the framework's own 64-bit value word. A count on a
//!   heap value is the framework's protection (`JSValueProtect`), so an
//!   `Owned` value, a member and a root handle each keep their target
//!   alive; immediates have no count.
//! - A realm is a global context of the isolate's context group, so
//!   values pass between the realms of one isolate.
//! - Host functions are objects of a callable class whose private data
//!   names the C-ABI function and its data word; the isolate and the
//!   host they get come from the engine call they run inside
//!   (`frame.rs`).
//! - A wrapper of a node in the document is protected while its realm
//!   lives; the wrappers of a detached tree keep one another alive in
//!   the framework's heap and die together once script holds none
//!   (`groups.rs`), after which the host's trial deletion frees the
//!   tree (`collect.rs`).
//! - Microtasks share the framework's queue, which it drains as the
//!   outermost call returns (`micro.rs`).
//! - Interrupts go through the framework's execution time limit, one
//!   of the undeclared entry points the architecture allows
//!   (`private.rs`).

mod collect;
mod engine;
mod ffi;
mod frame;
mod groups;
mod halt;
mod helpers;
mod isolate;
mod micro;
mod objects;
mod ops;
mod private;
mod protos;
mod strings;
mod value;
mod weak;
mod wrap;

pub use collect::{JscTracer, JscUnlinker};
pub use halt::{Flags, JscInterrupt};
pub use isolate::{Jsc, Realm};
pub use private::check as missing_entry_points;
pub use value::Value;
