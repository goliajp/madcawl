//! The dynamic atoms: names a page brings, interned in one table for
//! the whole process and freed when the last count on them goes.

use std::hash::{BuildHasher, RandomState};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use super::{Atom, generated::COUNT};
use crate::DomString;

/// A dynamic atom's string. The table owns it and nobody reaches it
/// but through the table, so its count never moves.
struct Text(DomString);

// SAFETY: the string is only touched with the table locked and is never
// cloned out of it, so no two threads ever touch its count
unsafe impl Send for Text {}

pub(super) struct Slot {
    key: Key,
    /// The hash of `key`, computed once.
    hash: u64,
    /// Made the first time it is read.
    text: Option<Text>,
    pub(super) count: u32,
}

/// A name, in place when it is short: most ids and class names are.
enum Key {
    Short(u8, [u8; SHORT_KEY]),
    Long(Box<[u8]>),
}

const SHORT_KEY: usize = 30;

impl Key {
    fn new(name: &[u8]) -> Key {
        if name.len() <= SHORT_KEY {
            let mut b = [0; SHORT_KEY];
            b[..name.len()].copy_from_slice(name);
            Key::Short(name.len() as u8, b)
        } else {
            Key::Long(name.into())
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Key::Short(n, b) => &b[..usize::from(*n)],
            Key::Long(b) => b,
        }
    }
}

/// A dynamic atom is its slot in the low bits and the slot's generation
/// above them. A slot is used again once its name is freed; the
/// generation moves each time, so an atom kept past its last holder reads
/// as gone instead of as the next name in its slot (until the generation
/// comes round again, 128 reuses later).
const SLOT_BITS: u32 = 24;
pub(super) const SLOT_MASK: u32 = (1 << SLOT_BITS) - 1;
const GEN_MASK: u32 = 0x7f;

/// An index entry with nothing in it, and one whose name was removed.
const EMPTY: u32 = 0;
const REMOVED: u32 = u32::MAX;

/// The names are the page's, so their hash is keyed per process
/// (`RandomState`): with a fixed one a page could pick names that all
/// collide. Each name is hashed once, when it is looked up; its slot
/// keeps the hash for inserting, growing and removing.
pub(super) struct Table {
    hasher: RandomState,
    /// Open addressing by hash, a power of two long: `EMPTY`, `REMOVED`,
    /// or a slot plus one.
    index: Vec<u32>,
    /// Entries of `index` that are not `EMPTY`.
    used: usize,
    slots: Vec<Option<Slot>>,
    /// Per slot, bumped when the slot is freed.
    gens: Vec<u32>,
    free: Vec<u32>,
}

impl Default for Table {
    fn default() -> Self {
        Table {
            hasher: RandomState::new(),
            index: Vec::new(),
            used: 0,
            slots: Vec::new(),
            gens: Vec::new(),
            free: Vec::new(),
        }
    }
}

static TABLE: LazyLock<Mutex<Table>> = LazyLock::new(Default::default);

// a panic while the lock is held happens before the table is changed,
// so a poisoned table is still whole
pub(super) fn table() -> MutexGuard<'static, Table> {
    TABLE.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Table {
    /// The slot holding `name`, whose hash is `h`.
    fn find(&self, h: u64, name: &[u8]) -> Option<u32> {
        let mask = self.index.len().checked_sub(1)?;
        let mut i = h as usize & mask;
        loop {
            match self.index[i] {
                EMPTY => return None,
                REMOVED => {}
                e => {
                    let s = self.slots[e as usize - 1]
                        .as_ref()
                        .expect("an indexed slot");
                    if s.hash == h && s.key.bytes() == name {
                        return Some(e - 1);
                    }
                }
            }
            i = (i + 1) & mask;
        }
    }

    /// Make room in the index for one more name, before its slot is
    /// filled.
    fn reserve(&mut self) {
        if (self.used + 1) * 2 > self.index.len() {
            let live = self.slots.len() - self.free.len();
            self.rebuild((live * 4).next_power_of_two().max(16));
        }
    }

    /// Put `slot` in the index, which does not have its name and has
    /// room for it.
    fn index_slot(&mut self, slot: u32, h: u64) {
        let mask = self.index.len() - 1;
        let mut i = h as usize & mask;
        while !matches!(self.index[i], EMPTY | REMOVED) {
            i = (i + 1) & mask;
        }
        self.used += usize::from(self.index[i] == EMPTY);
        self.index[i] = slot + 1;
    }

    fn rebuild(&mut self, len: usize) {
        self.index.clear();
        self.index.resize(len, EMPTY);
        self.used = 0;
        let mask = len - 1;
        for (slot, s) in self.slots.iter().enumerate() {
            let Some(s) = s else { continue };
            let mut i = s.hash as usize & mask;
            while self.index[i] != EMPTY {
                i = (i + 1) & mask;
            }
            self.index[i] = slot as u32 + 1;
            self.used += 1;
        }
    }

    /// The slot of `name`, made with no count if it is new.
    pub(super) fn slot_of(&mut self, name: &[u8]) -> u32 {
        let h = self.hasher.hash_one(name);
        if let Some(slot) = self.find(h, name) {
            return slot;
        }
        self.reserve();
        let entry = Slot {
            key: Key::new(name),
            hash: h,
            text: None,
            count: 0,
        };
        let slot = match self.free.pop() {
            Some(slot) => {
                self.slots[slot as usize] = Some(entry);
                slot
            }
            None => {
                let slot = u32::try_from(self.slots.len())
                    .ok()
                    .filter(|&s| s <= SLOT_MASK)
                    .expect("fewer than 2^24 dynamic atoms");
                self.slots.push(Some(entry));
                self.gens.push(0);
                slot
            }
        };
        self.index_slot(slot, h);
        slot
    }

    pub(super) fn entry(&mut self, slot: u32) -> &mut Slot {
        self.slots[slot as usize]
            .as_mut()
            .expect("a held atom is in the table")
    }

    pub(super) fn release(&mut self, slot: u32) {
        let e = self.entry(slot);
        e.count -= 1;
        if e.count == 0 {
            let e = self.slots[slot as usize].take().expect("just read");
            let mask = self.index.len() - 1;
            let mut i = e.hash as usize & mask;
            while self.index[i] != slot + 1 {
                i = (i + 1) & mask;
            }
            self.index[i] = REMOVED;
            self.gens[slot as usize] = (self.gens[slot as usize] + 1) & GEN_MASK;
            self.free.push(slot);
        }
    }
}

impl Table {
    pub(super) fn atom_of(&self, slot: u32) -> Atom {
        Atom(COUNT as u32 + (self.gens[slot as usize] << SLOT_BITS | slot))
    }

    /// The slot `atom` names, if its name is still in it.
    pub(super) fn live_slot(&self, atom: Atom) -> Option<u32> {
        let v = atom.0 - COUNT as u32;
        let slot = v & SLOT_MASK;
        let live =
            self.slots.get(slot as usize)?.is_some() && self.gens[slot as usize] == v >> SLOT_BITS;
        live.then_some(slot)
    }
}

/// The dynamic atom of `name` if some holder has it.
pub(super) fn lookup(name: &[u8]) -> Option<Atom> {
    let t = table();
    let h = t.hasher.hash_one(name);
    t.find(h, name).map(|s| t.atom_of(s))
}

/// Run `f` on the string of a dynamic atom, or `None` when no holder
/// has the atom any more.
pub(super) fn with_text<R>(atom: Atom, f: impl FnOnce(&DomString) -> R) -> Option<R> {
    let mut t = table();
    let slot = t.live_slot(atom)?;
    let e = t.slots[slot as usize].as_mut()?;
    let key = &e.key;
    let text = e
        .text
        .get_or_insert_with(|| Text(DomString::from_wtf8(key.bytes())));
    Some(f(&text.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HeldAtoms;

    #[test]
    fn the_last_release_frees_the_name_and_its_slot_is_used_again() {
        let mut t = Table::default();
        let a = t.slot_of(b"x-first");
        t.entry(a).count += 2;
        t.release(a);
        let h = t.hasher.hash_one(&b"x-first"[..]);
        assert_eq!(t.find(h, b"x-first"), Some(a));
        t.release(a);
        assert_eq!(t.find(h, b"x-first"), None);
        assert!(t.slots[a as usize].is_none());
        let b = t.slot_of(b"x-second");
        assert_eq!(b, a);
        assert_eq!(t.slots.len(), 1);
    }

    #[test]
    fn names_are_found_through_growth_and_removal() {
        let mut t = Table::default();
        let name = |p: &str, i: usize| format!("{p}-{i}").into_bytes();
        let slots: Vec<u32> = (0..2000)
            .map(|i| {
                let s = t.slot_of(&name("x", i));
                t.entry(s).count += 1;
                s
            })
            .collect();
        for &s in slots.iter().step_by(2) {
            t.release(s);
        }
        for (i, &s) in slots.iter().enumerate() {
            let n = name("x", i);
            let found = t.find(t.hasher.hash_one(&n[..]), &n);
            assert_eq!(found, (i % 2 == 1).then_some(s));
        }
        for i in 0..2000 {
            let s = t.slot_of(&name("y", i));
            t.entry(s).count += 1;
        }
        for i in (1..2000).step_by(2) {
            assert_eq!(t.slot_of(&name("x", i)), slots[i]);
        }
        assert_eq!(t.slots.len() - t.free.len(), 3000);
    }

    #[test]
    fn an_atom_kept_past_its_last_holder_does_not_read_as_the_next_name() {
        let mut t = Table::default();
        let a = t.slot_of(b"x-first");
        t.entry(a).count += 1;
        let first = t.atom_of(a);
        t.release(a);
        let b = t.slot_of(b"x-second");
        assert_eq!(b, a);
        let second = t.atom_of(b);
        assert_ne!(first, second);
        assert_eq!(t.live_slot(first), None);
        assert_eq!(t.live_slot(second), Some(b));
    }

    #[test]
    fn a_keep_that_panics_leaves_the_table_usable() {
        let h = HeldAtoms::new();
        let gone = h.hold(b"x-gone-before-keep");
        drop(h);
        assert!(std::panic::catch_unwind(|| HeldAtoms::new().keep(gone)).is_err());
        let after = HeldAtoms::new();
        let mut name = Vec::new();
        after.hold(b"x-after-the-panic").write_wtf8(&mut name);
        assert_eq!(name, b"x-after-the-panic");
    }
}
