//! Class lists, interned: a `class` value is split into atoms once per
//! distinct value, and each distinct list gets a number. Two elements
//! with the same number have exactly the same classes. The value's
//! string is shared too.

use std::collections::HashMap;

use madcawl_str::{Atom, DomString, HeldAtoms};

pub(crate) struct ClassLists {
    by_value: HashMap<Box<[u8]>, (DomString, u32)>,
    by_list: HashMap<Box<[Atom]>, u32>,
    /// List 0 is the empty one.
    lists: Vec<Box<[Atom]>>,
}

fn is_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | 0x0C | b'\r' | b' ')
}

impl ClassLists {
    pub(crate) fn new() -> Self {
        ClassLists {
            by_value: HashMap::new(),
            by_list: HashMap::new(),
            lists: vec![Box::new([])],
        }
    }

    /// The shared string of `value` and its list number.
    pub(crate) fn intern(&mut self, value: &[u8], atoms: &HeldAtoms) -> (DomString, u32) {
        if let Some((s, n)) = self.by_value.get(value) {
            return (s.clone(), *n);
        }
        let mut list: Vec<Atom> = Vec::new();
        for name in value.split(|&b| is_space(b)).filter(|n| !n.is_empty()) {
            let a = atoms.hold(name);
            if !list.contains(&a) {
                list.push(a);
            }
        }
        let n = if list.is_empty() {
            0
        } else if let Some(&n) = self.by_list.get(list.as_slice()) {
            n
        } else {
            let n = u32::try_from(self.lists.len()).expect("under 2^32 class lists");
            self.by_list.insert(list.clone().into_boxed_slice(), n);
            self.lists.push(list.into_boxed_slice());
            n
        };
        let s = DomString::from_wtf8(value);
        self.by_value.insert(value.into(), (s.clone(), n));
        (s, n)
    }

    pub(crate) fn list(&self, n: u32) -> &[Atom] {
        &self.lists[n as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_classes_get_the_same_number() {
        let atoms = HeldAtoms::new();
        let mut c = ClassLists::new();
        let (a, n) = c.intern(b"x-one  x-two", &atoms);
        let (_, m) = c.intern(b"\tx-one x-two x-one", &atoms);
        let (b, k) = c.intern(b"x-one  x-two", &atoms);
        assert_eq!(n, m);
        assert_eq!(n, k);
        assert_eq!(a, b);
        assert_eq!(c.list(n).len(), 2);
        assert_eq!(c.intern(b"  ", &atoms).1, 0);
        assert_ne!(c.intern(b"x-two", &atoms).1, n);
    }
}
