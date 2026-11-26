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
//!   sentinel [`Engine::thrown`]; exceptions of the host's own classes
//!   (`DOMException`) are objects the host makes. In the tables they are
//!   C-ABI functions of words ([`HostFn`]), given the isolate and the
//!   host in a [`Ctx`], and each member carries its Web IDL signature
//!   for a fast path ([`Signature`]).
//! - Each realm has an interface object and a prototype object per host
//!   class ([`Engine::interface_object`]); a wrapper inherits from its
//!   class's prototype object.
//! - The host holds script values through member handles (reported by
//!   `traverse`), root handles (never reported) and weak handles.
//! - A node has at most one wrapper, made lazily; its pointer is the
//!   node's wrapper word, weak until the engine asks for it to be
//!   preserved, after which the edge is counted and reported. The host
//!   reads it with [`Engine::wrapper_upgrade`], which refuses a wrapper
//!   that died or was found to be garbage.
//! - Strings are `DomString`s, passed by pointer; an engine string that
//!   is not a flat block is flattened by [`Engine::as_string`]. Property
//!   names are atoms.
//! - The host drives collection by deadline, in batches
//!   ([`Engine::collect_cycles`]), on its own clock ([`Host::now`]), and
//!   decides microtask checkpoints by [`Engine::in_script`].
//! - A browsing context's window is a WindowProxy whose target realm
//!   changes on navigation. An isolate is an agent cluster; several may
//!   share a thread.
//!
//! The binding code is generic over [`Engine`] and compiled once per
//! engine: dispatch is static, and a value is an 8-byte, bit-copyable
//! opaque word.
//!
//! How the final engine is joined to madcawl is not decided: as a Rust
//! crate implementing [`Engine`], or behind a C ABI with a thin crate
//! here implementing [`Engine`] over it. Nothing in this interface
//! assumes either: what compiled code calls directly (the class tables'
//! host functions, [`HostFn`]) is C-ABI and passes words, and the rest
//! is a trait an adapter can implement over foreign calls.
//!
//! [`trial`] is the trial deletion over a side graph that the stub
//! engine's collector runs, and that an adapter for a tracing engine
//! runs over detached trees.

mod class;
mod engine;
mod graph;
mod halt;
mod handle;
mod host;
mod script;
mod sig;
mod value;

pub mod trial;

pub use class::{
    Attribute, Bound, ClassDef, ClassId, Constant, Ctx, Getter, HostFn, HostOps, IndexGetter,
    Interceptors, Length, NamedGetter, Operation, Setter,
};
pub use engine::Engine;
pub use host::Host;
pub use graph::{Deadline, Step, Tracer, Unlinker};
pub use halt::{Control, Halt, Interrupt, Unsupported};
pub use handle::{Member, Root};
pub use script::{ModuleRequest, ScriptOrigin};
pub use sig::{AttributeSig, Effects, IdlType, Signature};
pub use value::{ErrorKind, HostRef, Key, Owned, Tree, ValueKind};
