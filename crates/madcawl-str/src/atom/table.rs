//! The dynamic atoms: names a page brings, interned in one table for
//! the whole process and freed when the last count on them goes.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use super::{Atom, generated::COUNT};
use crate::DomString;

/// A dynamic atom's string. The table owns it and nobody reaches it
/// but through the table, so its count never moves.
struct Text(DomString);

// SAFETY: the string is only touched with the table locked and is never
// cloned out of it, so no two threads ever touch its count
unsafe impl Send for Text {}

struct Slot {
    key: Box<[u8]>,
    text: Text,
    count: u32,
}

/// A dynamic atom is its slot in the low bits and the slot's generation
/// above them. A slot is used again once its name is freed; the
/// generation moves each time, so an atom kept past its last holder reads
/// as gone instead of as the next name in its slot (until the generation
/// comes round again, 128 reuses later).
const SLOT_BITS: u32 = 24;
const SLOT_MASK: u32 = (1 << SLOT_BITS) - 1;
const GEN_MASK: u32 = 0x7f;

/// The names are the page's, so the map's hash is keyed per process
/// (`RandomState`): with a fixed one a page could pick names that all
/// collide.
#[derive(Default)]
struct Table {
    map: HashMap<Box<[u8]>, u32>,
    slots: Vec<Option<Slot>>,
    /// Per slot, bumped when the slot is freed.
    gens: Vec<u32>,
    free: Vec<u32>,
}

static TABLE: LazyLock<Mutex<Table>> = LazyLock::new(Default::default);

// a panic while the lock is held happens before the table is changed,
// so a poisoned table is still whole
fn table() -> MutexGuard<'static, Table> {
    TABLE.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Table {
    /// The slot of `name`, made with no count if it is new.
    fn slot_of(&mut self, name: &[u8]) -> u32 {
        if let Some(&slot) = self.map.get(name) {
            return slot;
        }
        let entry = Slot {
            key: name.into(),
            text: Text(DomString::from_wtf8(name)),
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
        self.map.insert(name.into(), slot);
        slot
    }

    fn entry(&mut self, slot: u32) -> &mut Slot {
        self.slots[slot as usize]
            .as_mut()
            .expect("a held atom is in the table")
    }

    fn release(&mut self, slot: u32) {
        let e = self.entry(slot);
        e.count -= 1;
        if e.count == 0 {
            let e = self.slots[slot as usize].take().expect("just read");
            self.map.remove(&e.key);
            self.gens[slot as usize] = (self.gens[slot as usize] + 1) & GEN_MASK;
            self.free.push(slot);
        }
    }
}

impl Table {
    fn atom_of(&self, slot: u32) -> Atom {
        Atom(COUNT as u32 + (self.gens[slot as usize] << SLOT_BITS | slot))
    }

    /// The slot `atom` names, if its name is still in it.
    fn live_slot(&self, atom: Atom) -> Option<u32> {
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
    t.map.get(name).map(|&s| t.atom_of(s))
}

/// Run `f` on the string of a dynamic atom, or `None` when no holder
/// has the atom any more.
pub(super) fn with_text<R>(atom: Atom, f: impl FnOnce(&DomString) -> R) -> Option<R> {
    let t = table();
    let slot = t.live_slot(atom)?;
    let e = t.slots[slot as usize].as_ref()?;
    Some(f(&e.text.0))
}

/// The atoms one owner keeps alive: one count on each dynamic atom it
/// has asked for, given back when it is dropped. An atom stays the same
/// name for as long as some holder has it.
#[derive(Debug, Default)]
pub struct HeldAtoms {
    held: RefCell<HashSet<u32>>,
}

impl HeldAtoms {
    pub fn new() -> Self {
        Self::default()
    }

    /// The atom of `name`, WTF-8, held until `self` is dropped. A
    /// static name does not touch the table.
    pub fn hold(&self, name: &[u8]) -> Atom {
        if let Some(a) = Atom::lookup_static(name) {
            return a;
        }
        let mut t = table();
        let slot = t.slot_of(name);
        if self.held.borrow_mut().insert(slot) {
            t.entry(slot).count += 1;
        }
        t.atom_of(slot)
    }

    /// Hold `atom` too, as a document does with a name the parser gave
    /// it. Panics if no holder has the atom any more.
    pub fn keep(&self, atom: Atom) {
        if atom.is_static() {
            return;
        }
        let mut t = table();
        let Some(slot) = t.live_slot(atom) else {
            drop(t);
            panic!("keep of an atom no holder has");
        };
        if self.held.borrow_mut().insert(slot) {
            t.entry(slot).count += 1;
        }
    }

    /// How many dynamic atoms this holder keeps.
    pub fn len(&self) -> usize {
        self.held.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Drop for HeldAtoms {
    fn drop(&mut self) {
        let held = self.held.get_mut();
        if held.is_empty() {
            return;
        }
        let mut t = table();
        for slot in held.drain() {
            t.release(slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_release_frees_the_name_and_its_slot_is_used_again() {
        let mut t = Table::default();
        let a = t.slot_of(b"x-first");
        t.entry(a).count += 2;
        t.release(a);
        assert_eq!(t.map.get(&b"x-first"[..]), Some(&a));
        t.release(a);
        assert!(t.map.is_empty() && t.slots[a as usize].is_none());
        let b = t.slot_of(b"x-second");
        assert_eq!(b, a);
        assert_eq!(t.slots.len(), 1);
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
