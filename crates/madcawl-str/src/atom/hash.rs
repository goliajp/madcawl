//! The perfect hash over the static atoms. `tools/gen-atoms.py` builds
//! the tables with the same hash and mix; change both together.

use super::Atom;
use super::generated::{BUCKET_BITS, DISPLACE, NAMES, SLOT_BITS, SLOTS};

const FNV_OFFSET: u32 = 0x811C_9DC5;
const FNV_PRIME: u32 = 0x0100_0193;
const MIX: u32 = 0x9E37_79B1;

/// The hash of a name fed in pieces as it is read, so that the static
/// atom of a name costs a table probe and one comparison once the name
/// is complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameHash(u32);

impl Default for NameHash {
    fn default() -> Self {
        Self::new()
    }
}

impl NameHash {
    pub const fn new() -> Self {
        Self(FNV_OFFSET)
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u32::from(b)).wrapping_mul(FNV_PRIME);
        }
    }

    /// The static atom of `name`, which must be exactly the bytes fed.
    pub fn finish(self, name: &[u8]) -> Option<Atom> {
        find(self.0, name)
    }
}

pub(crate) const fn hash(name: &[u8]) -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0;
    while i < name.len() {
        h = (h ^ name[i] as u32).wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

pub(crate) const fn find(h: u32, name: &[u8]) -> Option<Atom> {
    let d = DISPLACE[(h >> (32 - BUCKET_BITS)) as usize];
    let mut x = (h ^ d).wrapping_mul(MIX);
    x ^= x >> 16;
    let i = SLOTS[(x & ((1 << SLOT_BITS) - 1)) as usize];
    if i == u16::MAX {
        return None;
    }
    let known = NAMES[i as usize].as_bytes();
    if known.len() != name.len() {
        return None;
    }
    let mut k = 0;
    while k < name.len() {
        if known[k] != name[k] {
            return None;
        }
        k += 1;
    }
    Some(Atom(i as u32))
}
