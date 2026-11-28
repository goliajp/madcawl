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

/// A pending promise and the functions that resolve and reject it,
/// each with a count for the holder.
pub struct Deferred<V: Copy> {
    pub promise: Owned<V>,
    pub resolve: Owned<V>,
    pub reject: Owned<V>,
}

/// Each method has a default for an engine without the builtin; the
/// methods take the engine's own types, so they are bound by `Engine`.
pub trait Builtins {
    /// The realm of the host function being run, the standard's current
    /// realm: `None` outside every host function, and once it is detached.
    fn current_realm(&self) -> Option<<Self as Engine>::Realm>
    where
        Self: Engine;

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

    /// An Array Iterator over `arr`, an array the host made, from
    /// `realm`'s `Array.prototype.values` as it was before any page
    /// script; thrown, with the exception registered, when that threw.
    /// The default calls `arr`'s @@iterator, for an engine that keeps no
    /// builtins aside.
    fn array_values<H: crate::Host<Self>>(
        &mut self,
        host: &mut H,
        realm: <Self as Engine>::Realm,
        arr: <Self as Engine>::Value,
    ) -> Owned<<Self as Engine>::Value>
    where
        Self: Engine,
    {
        let m = self.iterator_method(host, realm, arr);
        if Self::is_thrown(m.get()) {
            return m;
        }
        let it = self.call(host, m.get(), arr, &[]);
        self.release(m);
        it
    }

    /// The bytes `v` holds when it is an ArrayBuffer, or views when it is
    /// a typed array or a DataView (its own range of its buffer),
    /// borrowed until the next engine call: the standard's buffer source.
    /// `None` for any other value, and when script is halted; a detached
    /// buffer holds no bytes.
    fn buffer_source(
        &mut self,
        realm: <Self as Engine>::Realm,
        v: <Self as Engine>::Value,
    ) -> Option<&[u8]>
    where
        Self: Engine,
    {
        let _ = realm;
        self.typed_array(v).map(|(_, b)| &*b)
    }

    /// A new Uint8Array of `realm` holding `bytes`, or an ArrayBuffer
    /// when `buffer`, taken without a copy where the engine can; thrown,
    /// with nothing registered when script is halted, and with a
    /// RangeError when the engine cannot make one that long. An engine
    /// without typed arrays throws a TypeError.
    fn new_bytes(
        &mut self,
        realm: <Self as Engine>::Realm,
        bytes: Vec<u8>,
        buffer: bool,
    ) -> Owned<<Self as Engine>::Value>
    where
        Self: Engine,
    {
        let _ = (realm, bytes, buffer);
        self.throw_error(crate::ErrorKind::Type, "no typed arrays")
    }

    /// A new pending promise of `realm` with its resolving functions;
    /// `None` when script is halted, or the engine has no promises.
    fn new_promise(
        &mut self,
        realm: <Self as Engine>::Realm,
    ) -> Option<Deferred<<Self as Engine>::Value>>
    where
        Self: Engine,
    {
        let _ = realm;
        None
    }

    /// The own enumerable string-keyed properties of object `v`, as a
    /// record converts it: an array of their keys and values, each
    /// converted by ToString as it is read, alternating, in the order of
    /// `v`'s own keys. Thrown, with the exception registered, when a
    /// getter, a proxy trap or a conversion threw. An engine without
    /// property enumeration gives an empty array.
    fn own_entries<H: crate::Host<Self>>(
        &mut self,
        host: &mut H,
        realm: <Self as Engine>::Realm,
        v: <Self as Engine>::Value,
    ) -> Owned<<Self as Engine>::Value>
    where
        Self: Engine,
    {
        let _ = (host, v);
        self.new_array(realm, &[])
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
