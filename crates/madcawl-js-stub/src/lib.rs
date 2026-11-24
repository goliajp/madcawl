//! A script engine that runs no script.
//!
//! It implements the engine interface on the reference-counting model:
//! counted cells with the engine object header, wrappers that hold
//! their host object, member, root and weak handles, and a cycle
//! collector driven by the host's `traverse`, `unlink` and `can_skip`
//! by deadline in batches. Tests drive it as script would: read a
//! wrapper, read and write properties, call host functions, add own
//! properties, add listeners, drop references, collect. Bindings that
//! are only right under a tracing collector (an edge reported too few
//! or too many times) fail here.
//!
//! Handles keep to their isolate's thread:
//!
//! ```compile_fail
//! fn send<T: Send>(_: T) {}
//! send(madcawl_js::Member::<madcawl_js_stub::Stub>::empty());
//! ```
//!
//! ```compile_fail
//! fn send<T: Send>(_: T) {}
//! let mut e = madcawl_js_stub::Stub::new();
//! send(madcawl_js::Root::<madcawl_js_stub::Stub>::new(madcawl_js::Engine::retain(&mut e, <madcawl_js_stub::Stub as madcawl_js::Engine>::null())));
//! ```

mod cell;
mod collect;
mod engine;
mod halt;
mod isolate;
mod props;
mod strings;
mod value;
mod window;

pub use halt::StubInterrupt;

pub use collect::{CollectStats, StubTracer, StubUnlinker};
pub use isolate::{Realm, Stub};
pub use props::ClosureFn;
pub use value::Value;
