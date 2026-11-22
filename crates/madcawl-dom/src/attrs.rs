//! The attribute heap: blocks of 16-byte units, sized in powers of two,
//! with a free list per size. A block is a head unit and then one unit
//! per attribute; a node keeps the index of its block's head (0 is no
//! block).
//!
//! Head: `a` count, `b` capacity, `c` the `id` atom index plus one (low
//! half) and the class list number (high half).
//! Attribute: `a` local name atom, `b` namespace (high 16 bits) and
//! prefix atom index plus one (low 16 bits), `c` the value's
//! `DomString` block pointer.

use madcawl_html::AttrNamespace;
use madcawl_str::{Atom, DomString};

#[derive(Clone, Copy, Default)]
pub(crate) struct Unit {
    pub a: u32,
    pub b: u32,
    pub c: u64,
}

impl Unit {
    pub(crate) fn attr(
        local: Atom,
        ns: AttrNamespace,
        prefix: Option<Atom>,
        value: DomString,
    ) -> Unit {
        // the parser's prefixes are xlink, xml and xmlns, all static
        let prefix = prefix.map_or(0, |p| {
            u16::try_from(p.index() + 1).expect("a static prefix atom")
        });
        Unit {
            a: local.index(),
            b: (ns_bits(ns) << 16) | prefix as u32,
            c: value.into_raw().as_ptr() as u64,
        }
    }

    pub(crate) fn local(&self) -> Atom {
        Atom::from_index(self.a)
    }

    pub(crate) fn ns(&self) -> AttrNamespace {
        match self.b >> 16 {
            0 => AttrNamespace::None,
            1 => AttrNamespace::XLink,
            2 => AttrNamespace::Xml,
            _ => AttrNamespace::Xmlns,
        }
    }

    pub(crate) fn prefix(&self) -> Option<Atom> {
        let p = self.b & 0xFFFF;
        (p != 0).then(|| Atom::from_index(p - 1))
    }

    pub(crate) fn value(&self) -> &DomString {
        // SAFETY: an attribute unit's `c` holds a live block pointer, and
        // a `DomString` is `repr(transparent)` over that pointer
        unsafe { &*std::ptr::from_ref(&self.c).cast::<DomString>() }
    }

    /// Give back the count the unit's value holds.
    pub(crate) fn drop_value(&self) {
        // SAFETY: the unit owned one count on the block, given up here;
        // the unit is not read again
        drop(unsafe { DomString::from_raw(std::ptr::NonNull::new_unchecked(self.c as *mut u8)) });
    }
}

fn ns_bits(ns: AttrNamespace) -> u32 {
    match ns {
        AttrNamespace::None => 0,
        AttrNamespace::XLink => 1,
        AttrNamespace::Xml => 2,
        AttrNamespace::Xmlns => 3,
    }
}

pub(crate) struct AttrHeap {
    units: Vec<Unit>,
    /// Free blocks, by log2 of their capacity.
    free: Vec<Vec<u32>>,
}

impl AttrHeap {
    pub(crate) fn new() -> Self {
        AttrHeap {
            units: vec![Unit::default()],
            free: Vec::new(),
        }
    }

    /// A block for at least `n` attributes.
    pub(crate) fn alloc(&mut self, n: usize) -> u32 {
        let cap = n.max(1).next_power_of_two();
        let class = cap.trailing_zeros() as usize;
        if let Some(b) = self.free.get_mut(class).and_then(Vec::pop) {
            self.units[b as usize] = Unit {
                a: 0,
                b: cap as u32,
                c: 0,
            };
            return b;
        }
        let b = u32::try_from(self.units.len()).expect("the attribute heap under 2^32 units");
        self.units.push(Unit {
            a: 0,
            b: cap as u32,
            c: 0,
        });
        self.units.resize(self.units.len() + cap, Unit::default());
        b
    }

    /// Put block `b` on its free list; its values are already dropped.
    pub(crate) fn release(&mut self, b: u32) {
        let class = self.units[b as usize].b.trailing_zeros() as usize;
        if self.free.len() <= class {
            self.free.resize_with(class + 1, Vec::new);
        }
        self.free[class].push(b);
    }

    pub(crate) fn attrs(&self, b: u32) -> &[Unit] {
        let n = self.units[b as usize].a as usize;
        &self.units[b as usize + 1..b as usize + 1 + n]
    }

    /// Add `u` to block `b`, moving the block when it is full; the block
    /// the attribute ended up in.
    pub(crate) fn push(&mut self, b: u32, u: Unit) -> u32 {
        let head = self.units[b as usize];
        let b = if head.a < head.b {
            b
        } else {
            let nb = self.alloc(head.b as usize * 2);
            let (from, to) = (b as usize, nb as usize);
            self.units
                .copy_within(from + 1..from + 1 + head.a as usize, to + 1);
            self.units[to].a = head.a;
            self.units[to].c = head.c;
            self.release(b);
            nb
        };
        let h = &mut self.units[b as usize];
        let at = b as usize + 1 + h.a as usize;
        h.a += 1;
        self.units[at] = u;
        b
    }

    /// The `id` atom of block `b`.
    pub(crate) fn id(&self, b: u32) -> Option<Atom> {
        let v = self.units[b as usize].c as u32;
        (v != 0).then(|| Atom::from_index(v - 1))
    }

    pub(crate) fn set_id(&mut self, b: u32, id: Atom) {
        let h = &mut self.units[b as usize];
        h.c = h.c & !0xFFFF_FFFF | (id.index() + 1) as u64;
    }

    pub(crate) fn classes(&self, b: u32) -> u32 {
        (self.units[b as usize].c >> 32) as u32
    }

    pub(crate) fn set_classes(&mut self, b: u32, list: u32) {
        let h = &mut self.units[b as usize];
        h.c = h.c & 0xFFFF_FFFF | (list as u64) << 32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use madcawl_str::atom;

    #[test]
    fn a_full_block_moves_and_its_place_is_used_again() {
        let mut h = AttrHeap::new();
        let mut b = h.alloc(1);
        h.set_id(b, atom!("div"));
        for i in 0..5 {
            let v = DomString::from_latin1(format!("v{i}").as_bytes());
            b = h.push(b, Unit::attr(atom!("title"), AttrNamespace::None, None, v));
        }
        assert_eq!(h.attrs(b).len(), 5);
        assert_eq!(h.id(b), Some(atom!("div")));
        assert!(h.attrs(b)[4].value().eq_str("v4"));
        let again = h.alloc(1);
        assert_eq!(again, 1, "the first block, freed when it moved");
        for u in h.attrs(b) {
            u.drop_value();
        }
    }

    #[test]
    fn namespace_and_prefix_share_a_word() {
        let u = Unit::attr(
            atom!("href"),
            AttrNamespace::XLink,
            Some(atom!("xlink")),
            DomString::empty(),
        );
        assert_eq!(
            (u.local(), u.ns(), u.prefix()),
            (atom!("href"), AttrNamespace::XLink, Some(atom!("xlink")))
        );
        u.drop_value();
    }
}
