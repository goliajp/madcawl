//! a hasher for the cascade's per-element lookups keyed by numbers the
//! cascade or the atom table hands out (rule buckets, path keys by
//! parent and signature): every word goes through a folded multiply
//! with a per-process seed, several times faster than SipHash on these
//! one- and two-word keys. maps keyed by bytes a page writes keep SipHash

use std::hash::{BuildHasher, Hasher};

use super::intern::seed;

#[derive(Clone, Copy)]
pub(crate) struct Seeded(u64);

impl Default for Seeded {
    fn default() -> Seeded {
        Seeded(seed())
    }
}

impl BuildHasher for Seeded {
    type Hasher = Mix;

    fn build_hasher(&self) -> Mix {
        // the start and the multiplier come from the seed, mixed so that a
        // plain seed (1, say) gives a full-width multiplier
        Mix {
            h: fmix(self.0 ^ 0x243f_6a88_85a3_08d3),
            k: fmix(self.0) | 1,
        }
    }
}

pub(crate) struct Mix {
    h: u64,
    k: u64,
}

/// murmur3's finalizer: every bit of `h` reaches every bit of the result
fn fmix(mut h: u64) -> u64 {
    h = (h ^ (h >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
    h = (h ^ (h >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^ (h >> 33)
}

/// the high and low halves of `a * b` folded together
fn fold(a: u64, b: u64) -> u64 {
    let p = u128::from(a) * u128::from(b);
    (p as u64) ^ ((p >> 64) as u64)
}

impl Hasher for Mix {
    fn write(&mut self, bytes: &[u8]) {
        let (chunks, rest) = bytes.as_chunks::<8>();
        for c in chunks {
            self.write_u64(u64::from_le_bytes(*c));
        }
        let mut last = [0u8; 8];
        last[..rest.len()].copy_from_slice(rest);
        // the length too, so that trailing zero bytes are seen
        self.write_u64(u64::from_le_bytes(last) ^ (bytes.len() as u64) << 56);
    }

    fn write_u64(&mut self, v: u64) {
        self.h = fold(self.h ^ v, self.k);
    }

    fn write_u32(&mut self, v: u32) {
        self.write_u64(u64::from(v));
    }

    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }

    fn finish(&self) -> u64 {
        // the table takes the low bits: fold every bit into them
        fmix(self.h)
    }
}

/// a map keyed through `Seeded`
pub(crate) type Map<K, V> = std::collections::HashMap<K, V, Seeded>;

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::hash::BuildHasher;

    use super::*;

    #[test]
    fn every_byte_the_length_and_the_seed_count() {
        let s = Seeded(7);
        let base = vec![b'x'; 37];
        for i in 0..base.len() {
            let mut t = base.clone();
            t[i] = b'y';
            assert_ne!(s.hash_one(&base[..]), s.hash_one(&t[..]));
        }
        assert_ne!(s.hash_one(&b"ab"[..]), s.hash_one(&b"ab\0"[..]));
        assert_ne!(s.hash_one(1u32), Seeded(8).hash_one(1u32));
    }

    #[test]
    fn the_low_bits_spread_keys_that_differ_high() {
        // a table of 128 slots takes the low seven bits
        let s = Seeded::default();
        let slots = |keys: Vec<u64>| {
            keys.iter()
                .map(|&k| s.hash_one(k) & 127)
                .collect::<HashSet<_>>()
                .len()
        };
        assert!(slots((0..128).map(|i| i << 56).collect()) > 64);
        assert!(slots((0..128).map(|i| i << 32).collect()) > 64);
        assert!(slots((0..128).map(|i| i << 32 | 7).collect()) > 64);
    }

    #[test]
    fn no_difference_cancels_whatever_the_seed() {
        // eight pairs of words, each pair as it is or with bit 62 of the
        // first word and bit 3 (or bits 3 and 7) of the second flipped: a
        // multiply-rotate mix maps these 3^8 keys to one state per seed
        let family = |s: Seeded| {
            let mut out = HashSet::new();
            for n in 0..3usize.pow(8) {
                let mut h = s.build_hasher();
                let mut m = n;
                for i in 0..8u64 {
                    let (mut a, mut b) = (0x6161_6161_6161_6161 ^ i, 0x6262_6262_6262_6262 ^ i);
                    match m % 3 {
                        1 => (a, b) = (a ^ 1 << 62, b ^ 1 << 3),
                        2 => (a, b) = (a ^ 1 << 62, b ^ (1 << 3 | 1 << 7)),
                        _ => {}
                    }
                    m /= 3;
                    h.write_u64(a);
                    h.write_u64(b);
                }
                out.insert(h.finish());
            }
            out.len()
        };
        for seed in [1, 0x9e37_79b9_7f4a_7c15, seed()] {
            assert_eq!(family(Seeded(seed)), 3usize.pow(8), "seed {seed:x}");
        }
    }
}
