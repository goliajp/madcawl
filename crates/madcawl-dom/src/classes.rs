//! The id and class tables. Each distinct `id` value is an atom; each
//! distinct `class` value is split into atoms once and its list gets a
//! number, and two elements with the same number have exactly the same
//! classes. The value's string is shared too.
//!
//! Every entry is counted by the attributes holding it and given back
//! with the last of them, its atoms with it, so a page that keeps giving
//! an element new ids or classes uses only what its live values need.
//! What the tables hold is bounded by `Limits::tables`, apart from the
//! byte limit. A list number or atom given back may name another value
//! later: a reader keeps one only while it borrows the DOM.

use std::collections::HashMap;

use madcawl_str::{Atom, DomString, HeldAtoms, atom};

use crate::dom::{Dom, NodeId};

/// What an entry is counted as, besides its bytes.
const ENTRY: usize = 16;

struct Value {
    s: DomString,
    list: u32,
    refs: u32,
}

struct List {
    atoms: Box<[Atom]>,
    /// The values with this list; 0 for a free number.
    refs: u32,
}

pub(crate) struct Tables {
    /// The atoms of ids and class names, apart from the DOM's names so
    /// that giving one back leaves a name of the same spelling alone.
    atoms: HeldAtoms,
    /// By atom, the ids and lists holding it.
    atom_refs: HashMap<u32, u32>,
    /// By id atom, the elements with it and the value's length.
    ids: HashMap<u32, (u32, usize)>,
    by_value: HashMap<Box<[u8]>, Value>,
    by_list: HashMap<Box<[Atom]>, u32>,
    /// List 0 is the empty one.
    lists: Vec<List>,
    free: Vec<u32>,
    bytes: usize,
    limit: usize,
}

fn is_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | 0x0C | b'\r' | b' ')
}

impl Tables {
    pub(crate) fn new(limit: usize) -> Self {
        Tables {
            atoms: HeldAtoms::new(),
            atom_refs: HashMap::new(),
            ids: HashMap::new(),
            by_value: HashMap::new(),
            by_list: HashMap::new(),
            lists: vec![List {
                atoms: Box::new([]),
                refs: 1,
            }],
            free: Vec::new(),
            bytes: 0,
            limit,
        }
    }

    fn room(&self, n: usize) -> bool {
        self.limit - self.bytes >= n
    }

    fn hold(&mut self, name: &[u8]) -> Atom {
        let a = self.atoms.hold(name);
        *self.atom_refs.entry(a.index()).or_insert(0) += 1;
        a
    }

    fn let_go(&mut self, a: Atom) {
        let r = self.atom_refs.get_mut(&a.index()).expect("a held atom");
        *r -= 1;
        if *r == 0 {
            self.atom_refs.remove(&a.index());
            self.atoms.release(a);
        }
    }

    /// The atom of the non-empty id `v`, counted once more; `None` when
    /// it is new and the tables are full.
    pub(crate) fn id(&mut self, v: &[u8]) -> Option<Atom> {
        if let Some(a) = Atom::lookup(v)
            && let Some((refs, _)) = self.ids.get_mut(&a.index())
        {
            *refs += 1;
            return Some(a);
        }
        if !self.room(v.len() + ENTRY) {
            return None;
        }
        self.bytes += v.len() + ENTRY;
        let a = self.hold(v);
        self.ids.insert(a.index(), (1, v.len()));
        Some(a)
    }

    /// Count one more element with the id `a`, which one has already.
    pub(crate) fn retain_id(&mut self, a: Atom) {
        self.ids.get_mut(&a.index()).expect("a live id").0 += 1;
    }

    pub(crate) fn drop_id(&mut self, a: Atom) {
        let e = self.ids.get_mut(&a.index()).expect("a live id");
        e.0 -= 1;
        if e.0 == 0 {
            let len = e.1;
            self.ids.remove(&a.index());
            self.bytes -= len + ENTRY;
            self.let_go(a);
        }
    }

    /// The shared string of the class value `v` and its list number,
    /// counted once more; `None` when it is new and the tables are full.
    /// The empty value is not in the table.
    pub(crate) fn class(&mut self, v: &[u8]) -> Option<(DomString, u32)> {
        if v.is_empty() {
            return Some((DomString::empty(), 0));
        }
        if let Some(e) = self.by_value.get_mut(v) {
            e.refs += 1;
            return Some((e.s.clone(), e.list));
        }
        let mut names: Vec<&[u8]> = Vec::new();
        for name in v.split(|&b| is_space(b)).filter(|n| !n.is_empty()) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        // a new list is counted at 4 bytes an atom; lists are no more
        // than values, so this bounds them too
        if !self.room(v.len() + 4 * names.len() + 2 * ENTRY) {
            return None;
        }
        self.bytes += v.len() + ENTRY;
        let list = self.list_of(&names);
        let s = DomString::from_wtf8(v);
        let e = Value {
            s: s.clone(),
            list,
            refs: 1,
        };
        self.by_value.insert(v.into(), e);
        Some((s, list))
    }

    fn list_of(&mut self, names: &[&[u8]]) -> u32 {
        if names.is_empty() {
            return 0;
        }
        let atoms: Box<[Atom]> = names.iter().map(|n| self.hold(n)).collect();
        if let Some(&n) = self.by_list.get(&atoms) {
            for &a in &atoms {
                self.let_go(a);
            }
            self.lists[n as usize].refs += 1;
            return n;
        }
        self.bytes += 4 * atoms.len() + ENTRY;
        let l = List {
            atoms: atoms.clone(),
            refs: 1,
        };
        let n = match self.free.pop() {
            Some(n) => {
                self.lists[n as usize] = l;
                n
            }
            None => {
                self.lists.push(l);
                u32::try_from(self.lists.len() - 1).expect("under 2^32 class lists")
            }
        };
        self.by_list.insert(atoms, n);
        n
    }

    /// Count one element fewer with the class value `v`.
    pub(crate) fn drop_class(&mut self, v: &[u8]) {
        if v.is_empty() {
            return;
        }
        let e = self.by_value.get_mut(v).expect("a live class value");
        e.refs -= 1;
        if e.refs != 0 {
            return;
        }
        let list = e.list;
        self.by_value.remove(v);
        self.bytes -= v.len() + ENTRY;
        if list == 0 {
            return;
        }
        let l = &mut self.lists[list as usize];
        l.refs -= 1;
        if l.refs == 0 {
            let atoms = std::mem::take(&mut l.atoms);
            self.bytes -= 4 * atoms.len() + ENTRY;
            for &a in &atoms {
                self.let_go(a);
            }
            self.by_list.remove(&atoms);
            self.free.push(list);
        }
    }

    pub(crate) fn list(&self, n: u32) -> &[Atom] {
        &self.lists[n as usize].atoms
    }
}

impl Dom {
    /// The bytes the id and class tables hold, bounded by
    /// `Limits::tables`.
    pub fn table_bytes(&self) -> usize {
        self.tables.bytes
    }

    /// Count the table entry of `u`, an attribute of block `from` just
    /// copied into block `to`, for `to` too; the flags it gives.
    pub(crate) fn copy_entry(&mut self, from: u32, to: u32, u: crate::attrs::Unit) -> u32 {
        use crate::node::{HAS_CLASS, HAS_ID, HAS_STYLE};
        if u.ns() != madcawl_html::AttrNamespace::None {
            return 0;
        }
        match u.local() {
            a if a == atom!("class") => {
                let mut v = Vec::new();
                u.value().write_wtf8(&mut v);
                let (_, k) = self.tables.class(&v).expect("a value the table has");
                self.attrs.set_classes(to, k);
                HAS_CLASS
            }
            a if a == atom!("id") => match self.attrs.id(from) {
                Some(id) => {
                    self.tables.retain_id(id);
                    self.attrs.set_id(to, id);
                    HAS_ID
                }
                None => 0,
            },
            a if a == atom!("style") => HAS_STYLE,
            _ => 0,
        }
    }

    /// Give back the table entries element `i`'s attributes hold.
    pub(crate) fn drop_entries(&mut self, i: u32) {
        let b = self.n(i).attrs;
        if b == 0 {
            return;
        }
        if let Some(id) = self.attrs.id(b) {
            self.tables.drop_id(id);
        }
        if let Some(v) = self.attribute(NodeId::at(i), atom!("class")) {
            let mut buf = Vec::new();
            v.write_wtf8(&mut buf);
            self.tables.drop_class(&buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_classes_get_the_same_number() {
        let mut c = Tables::new(1 << 20);
        let (a, n) = c.class(b"x-one  x-two").unwrap();
        let (_, m) = c.class(b"\tx-one x-two x-one").unwrap();
        let (b, k) = c.class(b"x-one  x-two").unwrap();
        assert_eq!(n, m);
        assert_eq!(n, k);
        assert_eq!(a, b);
        assert_eq!(c.list(n).len(), 2);
        assert_eq!(c.class(b"  ").unwrap().1, 0);
        assert_ne!(c.class(b"x-two").unwrap().1, n);
    }

    #[test]
    fn the_last_value_gives_back_its_list_and_atoms() {
        let mut c = Tables::new(1 << 20);
        let (_, n) = c.class(b"x-gone-a x-gone-b").unwrap();
        c.class(b"x-gone-a  x-gone-b").unwrap();
        c.drop_class(b"x-gone-a x-gone-b");
        assert_eq!(c.list(n).len(), 2, "the other spelling keeps the list");
        c.drop_class(b"x-gone-a  x-gone-b");
        assert_eq!(c.bytes, 0);
        assert_eq!(Atom::lookup(b"x-gone-a"), None);
        let id = c.id(b"x-gone-a").unwrap();
        c.retain_id(id);
        c.drop_id(id);
        assert!(c.bytes > 0);
        c.drop_id(id);
        assert_eq!((c.bytes, c.atom_refs.len()), (0, 0));
        let (_, again) = c.class(b"x-new").unwrap();
        assert_eq!(again, n, "the number is used again");
    }

    #[test]
    fn a_full_table_refuses_new_values_only() {
        let mut c = Tables::new(100);
        assert!(c.id(b"x-full-one").is_some());
        assert!(c.class(&[b'k'; 80]).is_none());
        assert!(c.id(b"x-full-one").is_some(), "a value it has is counted");
    }
}
