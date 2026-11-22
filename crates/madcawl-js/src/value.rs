//! Values as the host sees them, and who owns their counts.

use std::num::NonZeroUsize;

use madcawl_str::{Atom, DomString};

/// A value that carries one count. A host function returns one, an
/// engine call that makes or reads a value returns one, and the holder
/// either stores it in a handle, returns it, or gives it back with
/// [`crate::Engine::release`]. A plain `E::Value` is borrowed: valid for
/// the call it was passed to, with no count of its own.
#[must_use = "an owned value carries a count: store it, return it or release it"]
#[repr(transparent)]
#[derive(Debug, PartialEq, Eq)]
pub struct Owned<V: Copy>(V);

impl<V: Copy> Owned<V> {
    /// # Safety
    /// The caller holds one count on `v` (or `v` is not counted) and
    /// gives it to the new `Owned`.
    pub unsafe fn from_raw(v: V) -> Self {
        Owned(v)
    }

    /// The value, borrowed: the count stays with `self`.
    pub fn get(&self) -> V {
        self.0
    }

    /// The value with its count, now the caller's.
    pub fn into_raw(self) -> V {
        self.0
    }
}

/// A property key. Names are atoms, one interning table shared with
/// the DOM; a string key that names no atom is interned by the engine.
#[derive(Clone, Copy, Debug)]
pub enum Key<'a> {
    Name(Atom),
    Index(u32),
    Str(&'a DomString),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Undefined,
    Null,
    Boolean,
    Number,
    String,
    Object,
    Function,
}

/// The errors a host function throws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Type,
    Range,
    Syntax,
    Reference,
    /// A `DOMException` with this name.
    Dom(&'static str),
}

/// A host object: the address of its engine object header. A node's
/// address is fixed from creation to release, so the engine may keep
/// it in a wrapper for as long as the wrapper holds the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HostRef(NonZeroUsize);

impl HostRef {
    pub fn new(addr: usize) -> Option<HostRef> {
        NonZeroUsize::new(addr).map(HostRef)
    }

    pub fn addr(self) -> usize {
        self.0.get()
    }
}
