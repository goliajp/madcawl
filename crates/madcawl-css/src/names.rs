//! The names a stylesheet's selectors use (element and attribute names,
//! classes, ids, namespace prefixes and URLs), interned in a table of
//! the stylesheet's own. A worker thread can parse without touching the
//! process's atom table; the main thread turns every name into an atom
//! in one pass (`Names::atoms`), one lookup per distinct name.

use std::collections::HashMap;

use madcawl_str::{Atom, HeldAtoms};

/// An index into a stylesheet's `Names`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(u32);

impl Name {
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn from_index(i: usize) -> Name {
        Name(i as u32)
    }
}

#[derive(Debug, Default)]
pub struct Names {
    text: String,
    spans: Vec<(u32, u32)>,
    index: HashMap<Box<str>, Name>,
}

impl Names {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, name: &str) -> Name {
        if let Some(&n) = self.index.get(name) {
            return n;
        }
        let n = Name(self.spans.len() as u32);
        let start = self.text.len() as u32;
        self.text.push_str(name);
        self.spans.push((start, self.text.len() as u32));
        self.index.insert(name.into(), n);
        n
    }

    pub fn get(&self, name: Name) -> &str {
        let (s, e) = self.spans[name.index()];
        &self.text[s as usize..e as usize]
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The atom of every name, indexed by `Name::index`, held by
    /// `held`. On the main thread, which owns the atom table.
    pub fn atoms(&self, held: &HeldAtoms) -> Vec<Atom> {
        (0..self.spans.len())
            .map(|i| held.hold(self.get(Name(i as u32)).as_bytes()))
            .collect()
    }
}
