//! Atoms: names interned as 32-bit indices, compared as integers.
//!
//! Indices below `STATIC_ATOMS` are the static atoms, fixed at compile
//! time (`tools/gen-atoms.py`): the element and attribute names of
//! HTML, SVG and MathML, and event types. Their strings are static
//! blocks in read-only data, so any thread can read them. The rest are
//! dynamic atoms in one table for the process, each alive while some
//! `HeldAtoms` has it.

#[rustfmt::skip]
mod generated;
mod hash;
mod statics;
mod table;

pub use hash::NameHash;
pub use table::HeldAtoms;

use crate::{DomString, StaticStr};

/// How many static atoms there are; their indices are `0..STATIC_ATOMS`.
pub const STATIC_ATOMS: usize = generated::COUNT;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Atom(u32);

impl Atom {
    pub const fn index(self) -> u32 {
        self.0
    }

    pub const fn is_static(self) -> bool {
        (self.0 as usize) < STATIC_ATOMS
    }

    /// The static atom spelled exactly `name`.
    pub const fn lookup_static(name: &[u8]) -> Option<Atom> {
        hash::find(hash::hash(name), name)
    }

    /// The atom spelled `name`, if it is static or some holder has it.
    pub fn lookup(name: &[u8]) -> Option<Atom> {
        Self::lookup_static(name).or_else(|| table::lookup(name))
    }

    /// The string of a static atom, readable from any thread.
    pub fn as_static(self) -> Option<StaticStr> {
        self.is_static().then(|| statics::get(self.0 as usize))
    }

    /// The atom's string: the static block itself, or a copy of a
    /// dynamic one. Panics if no holder has the atom any more.
    pub fn to_dom_string(self) -> DomString {
        match self.as_static() {
            Some(s) => s.into(),
            None => table::with_text(self, DomString::copy).expect("a held atom"),
        }
    }

    /// Append the name as WTF-8. Panics if no holder has the atom any
    /// more.
    pub fn write_wtf8(self, out: &mut Vec<u8>) {
        match self.as_static() {
            Some(s) => s.write_wtf8(out),
            None => table::with_text(self, |s| s.write_wtf8(out)).expect("a held atom"),
        }
    }
}

impl std::fmt::Debug for Atom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut v = Vec::new();
        let name = match self.as_static() {
            Some(s) => {
                s.write_wtf8(&mut v);
                true
            }
            None => table::with_text(*self, |s| s.write_wtf8(&mut v)).is_some(),
        };
        if name {
            write!(f, "Atom({} {:?})", self.0, String::from_utf8_lossy(&v))
        } else {
            write!(f, "Atom({} released)", self.0)
        }
    }
}

/// The static atom spelled by a string literal, checked at compile time.
#[macro_export]
macro_rules! atom {
    ($name:literal) => {{
        const ATOM: $crate::Atom = match $crate::Atom::lookup_static($name.as_bytes()) {
            Some(a) => a,
            None => panic!(concat!("not a static atom: ", $name)),
        };
        ATOM
    }};
}

#[cfg(test)]
mod tests;
