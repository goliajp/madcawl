//! What the host reads and does through a realm's own builtins: those
//! taken when the realm was made, before any page script, so that
//! nothing a page replaced or wrapped runs.

use madcawl_str::DomString;

use crate::engine::Engine;
use crate::value::{ArrayType, Owned};

/// What an error object says of itself, read from its own data
/// properties: each string at most the code units asked for, plus one
/// when it was longer. Where it was thrown is the engine's own record,
/// under whatever names the engine gives it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ErrorInfo {
    pub name: Option<DomString>,
    pub message: Option<DomString>,
    pub source: Option<DomString>,
    /// 1-based; 0 when unknown
    pub line: u32,
    pub column: u32,
}

/// Each method has a default for an engine without the builtin; the
/// methods take the engine's own types, so they are bound by `Engine`.
pub trait Builtins {
    /// The element type and the bytes (its own range of its buffer) of
    /// `v` when it is a typed array, borrowed until the next engine
    /// call; `None` for any other value. An engine without typed arrays
    /// has none.
    fn typed_array(&mut self, v: <Self as Engine>::Value) -> Option<(ArrayType, &mut [u8])>
    where
        Self: Engine,
    {
        let _ = v;
        None
    }

    /// What `v` says of itself when it is an error object (not a proxy
    /// of one), or the text of `v` when it is a string (as `message`),
    /// each string cut to `max + 1` code units; read without running
    /// script: no getter, no proxy trap, no `toString`. `None` for any
    /// other value, and when script is halted.
    fn error_info(&mut self, realm: <Self as Engine>::Realm, v: <Self as Engine>::Value, max: usize) -> Option<ErrorInfo>
    where
        Self: Engine,
    {
        let _ = (realm, v, max);
        None
    }

    /// GetMethod(`v`, @@iterator) for an object `v`: the value of its
    /// `Symbol.iterator` property, which may run script (a getter, a
    /// proxy trap); thrown, with the exception registered, when that
    /// threw. An engine without symbols has none: undefined.
    fn iterator_method<H: crate::Host<Self>>(
        &mut self,
        host: &mut H,
        realm: <Self as Engine>::Realm,
        v: <Self as Engine>::Value,
    ) -> Owned<<Self as Engine>::Value>
    where
        Self: Engine,
    {
        let _ = (host, realm, v);
        self.retain(Self::undefined())
    }

    /// Make `name` of `obj` an enumerable accessor property of `get` and
    /// `set` (borrowed functions; undefined for no setter), not
    /// configurable when `unforgeable`. `false` when it threw.
    fn define_accessor(
        &mut self,
        realm: <Self as Engine>::Realm,
        obj: <Self as Engine>::Value,
        name: &str,
        get: <Self as Engine>::Value,
        set: <Self as Engine>::Value,
        unforgeable: bool,
    ) -> bool
    where
        Self: Engine;
}
