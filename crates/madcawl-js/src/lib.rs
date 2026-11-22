//! The interface between madcawl and its script engine.
//!
//! The shape is that of a reference-counting engine with a cycle
//! collector, and an engine of another kind is adapted to it, not the
//! other way round:
//!
//! - Host objects are the DOM nodes themselves. A node starts with the
//!   engine's 8-byte object header (count, type tag, flags), and nodes
//!   are vertices of the collector's graph whether or not they have a
//!   wrapper.
//! - Host classes are static tables made from Web IDL ([`ClassDef`]):
//!   class ids numbered so that "is an instance of" is a range compare,
//!   a parent, attributes, operations, constants, interceptors, and the
//!   collector's callbacks ([`HostOps`]: `traverse`, `unlink`,
//!   `can_skip`, `wrapper_needs_preserve`, `wrapper_died`).
//! - Host functions borrow their arguments and transfer their result.
//!   On error they register the exception in the context and return the
//!   sentinel [`Engine::thrown`].
//! - The host holds script values through member handles (reported by
//!   `traverse`), root handles (never reported) and weak handles.
//! - A node has at most one wrapper, made lazily; its pointer is the
//!   node's wrapper word, weak until the engine asks for it to be
//!   preserved, after which the edge is counted and reported.
//! - Strings are `DomString`s, passed by pointer; property names are
//!   atoms.
//! - The host drives collection by deadline, in batches
//!   ([`Engine::collect_cycles`]), and decides microtask checkpoints.
//!
//! The binding code is generic over [`Engine`] and compiled once per
//! engine: dispatch is static, and a value is an 8-byte, bit-copyable
//! opaque word.
//!
//! [`trial`] is the trial deletion over a side graph that the stub
//! engine's collector runs, and that an adapter for a tracing engine
//! runs over detached trees.

mod class;
mod engine;
mod handle;
mod value;

pub mod trial;

pub use class::{
    Attribute, ClassDef, ClassId, Constant, Getter, HostFn, HostOps, IndexGetter, Interceptors,
    Length, Operation, Setter,
};
pub use engine::{
    Control, Deadline, Engine, Host, Interrupt, ModuleRequest, ScriptOrigin, Step, Tracer,
    Unlinker,
};
pub use handle::{Member, Root};
pub use value::{ErrorKind, HostRef, Key, Owned, ValueKind};
