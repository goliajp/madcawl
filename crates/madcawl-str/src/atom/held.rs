//! What one owner of atoms holds: a count on each dynamic atom it has
//! asked for, and the names it asked for lately.

use std::cell::RefCell;
use std::collections::HashSet;
use std::hash::{BuildHasherDefault, Hasher};

use super::table::{SLOT_MASK, table};
use super::{Atom, generated::COUNT};

/// The hash of a held atom's index. Indices are handed out by the
/// table, not chosen by a page, so a multiply spreads them well enough.
#[derive(Default)]
struct IndexHash(u64);

impl Hasher for IndexHash {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u32(u32::from(b));
        }
    }
    fn write_u32(&mut self, i: u32) {
        self.0 = (self.0 ^ u64::from(i)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

/// Names this holder asked for lately, by the low bits of their hash:
/// a name asked for again is found without the table's lock. A page
/// that makes names collide here only makes them miss.
const RECENT: usize = 64;
const RECENT_LEN: usize = 24;

#[derive(Clone, Copy)]
struct Recent {
    len: u8,
    name: [u8; RECENT_LEN],
    atom: Atom,
}

#[derive(Default)]
struct Held {
    /// The index of each dynamic atom held, its generation included, so
    /// that holding it again is answered here.
    atoms: HashSet<u32, BuildHasherDefault<IndexHash>>,
    recent: Option<Box<[Recent; RECENT]>>,
}

impl Held {
    fn recent(&self, h: u32, name: &[u8]) -> Option<Atom> {
        let r = &self.recent.as_ref()?[h as usize % RECENT];
        (usize::from(r.len) == name.len() && &r.name[..name.len()] == name).then_some(r.atom)
    }

    fn remember(&mut self, h: u32, name: &[u8], atom: Atom) {
        if name.len() > RECENT_LEN {
            return;
        }
        let recent = self.recent.get_or_insert_with(|| {
            Box::new(
                [Recent {
                    len: u8::MAX,
                    name: [0; RECENT_LEN],
                    atom,
                }; RECENT],
            )
        });
        let r = &mut recent[h as usize % RECENT];
        r.len = name.len() as u8;
        r.name[..name.len()].copy_from_slice(name);
        r.atom = atom;
    }
}

/// The atoms one owner keeps alive: one count on each dynamic atom it
/// has asked for, given back when it is dropped. An atom stays the same
/// name for as long as some holder has it.
#[derive(Default)]
pub struct HeldAtoms {
    held: RefCell<Held>,
}

impl std::fmt::Debug for HeldAtoms {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeldAtoms")
            .field("len", &self.len())
            .finish()
    }
}

impl HeldAtoms {
    pub fn new() -> Self {
        Self::default()
    }

    /// The atom of `name`, WTF-8, held until `self` is dropped. A
    /// static name does not touch the table.
    pub fn hold(&self, name: &[u8]) -> Atom {
        let h = super::hash::hash(name);
        if let Some(a) = super::hash::find(h, name) {
            return a;
        }
        let mut held = self.held.borrow_mut();
        if let Some(a) = held.recent(h, name) {
            return a;
        }
        let mut t = table();
        let slot = t.slot_of(name);
        let atom = t.atom_of(slot);
        if held.atoms.insert(atom.0) {
            t.entry(slot).count += 1;
        }
        drop(t);
        held.remember(h, name, atom);
        atom
    }

    /// Hold `atom` too, as a document does with a name the parser gave
    /// it. Panics if no holder has the atom any more.
    pub fn keep(&self, atom: Atom) {
        if atom.is_static() {
            return;
        }
        let mut held = self.held.borrow_mut();
        if held.atoms.contains(&atom.0) {
            return;
        }
        let mut t = table();
        let Some(slot) = t.live_slot(atom) else {
            drop(t);
            panic!("keep of an atom no holder has");
        };
        held.atoms.insert(atom.0);
        t.entry(slot).count += 1;
    }

    /// How many dynamic atoms this holder keeps.
    pub fn len(&self) -> usize {
        self.held.borrow().atoms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Drop for HeldAtoms {
    fn drop(&mut self) {
        let held = &mut self.held.get_mut().atoms;
        if held.is_empty() {
            return;
        }
        let mut t = table();
        for atom in held.drain() {
            t.release((atom - COUNT as u32) & SLOT_MASK);
        }
    }
}
