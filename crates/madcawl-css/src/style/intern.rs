//! The table that gives equal computed values one place. Each value's
//! hash is kept beside it, so a value is hashed once when it is interned
//! and never again when the table grows.
//!
//! Values are hashed with a multiply-rotate mix, which is several times
//! cheaper than SipHash over the kilobyte of fields a value feeds it but
//! does not resist collisions made on purpose, and a page's stylesheets
//! decide the values. So a lookup that probes more than `FLOOD` slots
//! switches the table, once, to a keyed SipHash and rehashes what it
//! holds: no lookup ever takes more than `FLOOD` probes under the mix.
//! After the switch probes have no cap; colliding values would have to be
//! found without the key.

use std::hash::{BuildHasher, Hash, Hasher, RandomState};
use std::sync::OnceLock;

use super::computed::ComputedStyle;

/// Probes past which a lookup takes the table to be flooded. With the
/// table at most half full, values that hash evenly take more than this
/// very rarely, and a needless switch only makes hashing slower.
const FLOOD: usize = 32;

#[derive(Default)]
pub(crate) struct Interner {
    /// 1 + the index in the table of values, 0 for an empty slot; a power
    /// of two long and at most half full
    slots: Vec<u32>,
    /// By index in the table of values, its hash
    hashes: Vec<u64>,
    /// Set when a lookup probed past `FLOOD`
    keyed: Option<RandomState>,
    /// Every value hashes the same under the mix
    #[cfg(test)]
    collide: bool,
}

impl Interner {
    /// The index in `table` of a value equal to `s`, which is added at
    /// the end when there is none. `table` holds exactly the values this
    /// interner was given that were not found.
    pub(crate) fn intern(&mut self, table: &mut Vec<ComputedStyle>, s: ComputedStyle) -> u32 {
        let h = self.hash(&s);
        let slot = match self.find(table, &s, h) {
            Ok(k) => return k,
            Err(Some(slot)) if (table.len() + 1) * 2 <= self.slots.len() => slot,
            Err(None) if !self.slots.is_empty() => {
                self.keyed = Some(RandomState::new());
                self.hashes = table.iter().map(|v| self.hash(v)).collect();
                self.rebuild(self.slots.len());
                return self.intern(table, s);
            }
            Err(_) => {
                self.rebuild((self.slots.len() * 2).max(16));
                self.free(h)
            }
        };
        let k = table.len() as u32;
        table.push(s);
        self.hashes.push(h);
        self.slots[slot] = k + 1;
        k
    }

    fn hash(&self, s: &ComputedStyle) -> u64 {
        match &self.keyed {
            Some(k) => k.hash_one(s),
            #[cfg(test)]
            None if self.collide => 7,
            None => {
                let mut m = Mix(seed());
                s.hash(&mut m);
                m.0
            }
        }
    }

    // the found value's index; or the first empty slot on the way, none
    // when there are no slots or the lookup probed past `FLOOD` under the mix
    fn find(
        &self,
        table: &[ComputedStyle],
        s: &ComputedStyle,
        h: u64,
    ) -> Result<u32, Option<usize>> {
        if self.slots.is_empty() {
            return Err(None);
        }
        let mask = self.slots.len() - 1;
        let mut at = start(h, mask);
        for step in 1.. {
            let k = self.slots[at];
            if k == 0 {
                return Err(Some(at));
            }
            let k = k - 1;
            if self.hashes[k as usize] == h && table[k as usize] == *s {
                return Ok(k);
            }
            if step >= FLOOD && self.keyed.is_none() {
                return Err(None);
            }
            at = (at + step) & mask;
        }
        unreachable!()
    }

    // the first empty slot for `h`; the table has one
    fn free(&self, h: u64) -> usize {
        let mask = self.slots.len() - 1;
        let mut at = start(h, mask);
        let mut step = 1;
        while self.slots[at] != 0 {
            at = (at + step) & mask;
            step += 1;
        }
        at
    }

    fn rebuild(&mut self, len: usize) {
        self.slots.clear();
        self.slots.resize(len, 0);
        for k in 0..self.hashes.len() {
            let at = self.free(self.hashes[k]);
            self.slots[at] = k as u32 + 1;
        }
    }
}

// the top bits: the mix ends in a multiply, which leaves them the best mixed
fn start(h: u64, mask: usize) -> usize {
    (h >> 32) as usize & mask
}

// a seed per process, so that values colliding under the mix cannot be worked out ahead of time
fn seed() -> u64 {
    static SEED: OnceLock<u64> = OnceLock::new();
    *SEED.get_or_init(|| RandomState::new().hash_one(0u8))
}

struct Mix(u64);

impl Hasher for Mix {
    fn write(&mut self, bytes: &[u8]) {
        let (chunks, rest) = bytes.as_chunks::<8>();
        for c in chunks {
            self.write_u64(u64::from_le_bytes(*c));
        }
        if !rest.is_empty() {
            let mut v = [0u8; 8];
            v[..rest.len()].copy_from_slice(rest);
            self.write_u64(u64::from_le_bytes(v));
        }
    }

    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn write_u8(&mut self, v: u8) {
        self.write_u64(u64::from(v));
    }

    fn write_u16(&mut self, v: u16) {
        self.write_u64(u64::from(v));
    }

    fn write_u32(&mut self, v: u32) {
        self.write_u64(u64::from(v));
    }

    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }

    fn write_isize(&mut self, v: isize) {
        self.write_u64(v as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn value(i: i32) -> ComputedStyle {
        let mut s = ComputedStyle::initial().clone();
        Arc::make_mut(&mut s.place).z_index = Some(i);
        s
    }

    fn check(mut m: Interner, n: i32) -> Interner {
        let mut table = Vec::new();
        for i in 0..n {
            assert_eq!(m.intern(&mut table, value(i)), i as u32);
        }
        for i in (0..n).rev() {
            assert_eq!(m.intern(&mut table, value(i)), i as u32);
        }
        assert_eq!(table.len(), n as usize);
        assert!(m.slots.len() >= 2 * table.len());
        m
    }

    #[test]
    fn equal_values_keep_their_place_as_the_table_grows() {
        let m = check(Interner::default(), 5000);
        assert!(m.keyed.is_none());
    }

    #[test]
    fn a_flood_of_colliding_values_switches_to_the_keyed_hash() {
        let flood = Interner {
            collide: true,
            ..Interner::default()
        };
        // under the mix every value would land in one run of slots, and
        // each lookup would compare it with all the values before it
        let m = check(flood, 5000);
        assert!(m.keyed.is_some());
    }
}
