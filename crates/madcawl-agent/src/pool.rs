//! The names and values records point at: each distinct string stored
//! once, counted, and freed when no record uses it.
//!
//! The strings lie end to end in one buffer and are found through an
//! open-addressing table of ids (linear probing, deletion by shifting
//! back), so a string costs its bytes and a few words, not an
//! allocation of its own. A freed string's bytes stay in the buffer
//! until the dead bytes outweigh the live ones; then the live strings
//! are packed again, so the buffer is at most about twice what is live.

use std::hash::BuildHasher;

/// Dead bytes tolerated before packing, so a small pool is never packed
const SLACK: usize = 64 << 10;
/// A span's length bits; its start has the other 40, a terabyte
const LEN_BITS: u32 = 24;
const LEN_MASK: u64 = (1 << LEN_BITS) - 1;

pub(crate) struct Pool {
    text: String,
    /// By id: where the string starts in `text` (high 40 bits) and its
    /// length (low 24 bits; the tree caps what it puts in at `MAX_NAME`)
    spans: Vec<u64>,
    counts: Vec<u32>,
    hashes: Vec<u32>,
    /// Id + 1 per slot, 0 for an empty one; a power of two long, at most half full
    slots: Vec<u32>,
    free: Vec<u32>,
    live: usize,
    seed: u64,
}

/// A multiply-rotate hash over eight bytes at a time, seeded per pool
/// so a page cannot choose strings that collide, and mixed at the end:
/// a byte in the last word's top half moves only the product's top bits,
/// which the slot would never see.
fn hash(seed: u64, s: &str) -> u32 {
    let mut h = seed ^ (s.len() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let (words, rest) = s.as_bytes().as_chunks::<8>();
    let mut last = [0u8; 8];
    last[..rest.len()].copy_from_slice(rest);
    for w in words.iter().chain([&last]) {
        h = (h.rotate_left(5) ^ u64::from_le_bytes(*w)).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    h ^= h >> 32;
    h = h.wrapping_mul(0xd6e8_feb8_6659_fd93);
    h ^= h >> 29;
    h as u32
}

impl Pool {
    pub fn new() -> Self {
        Pool {
            text: String::new(),
            spans: vec![0],
            counts: vec![0],
            hashes: vec![0],
            slots: Vec::new(),
            free: Vec::new(),
            live: 0,
            seed: std::collections::hash_map::RandomState::new().hash_one(0u64),
        }
    }

    fn mask(&self) -> usize {
        self.slots.len() - 1
    }

    /// The slot holding `s` (hashed to `h`), or the empty slot it would go in.
    fn find(&self, s: &str, h: u32) -> (usize, bool) {
        let mut i = h as usize & self.mask();
        loop {
            match self.slots[i] {
                0 => return (i, false),
                e => {
                    let id = (e - 1) as usize;
                    if self.hashes[id] == h && self.get(id as u32) == s {
                        return (i, true);
                    }
                }
            }
            i = (i + 1) & self.mask();
        }
    }

    /// The id of `s`, counted once more; 0 is the empty string.
    pub fn intern(&mut self, s: &str) -> u32 {
        if s.is_empty() {
            return 0;
        }
        if 2 * (self.spans.len() - self.free.len()) >= self.slots.len() {
            self.grow();
        }
        let h = hash(self.seed, s);
        let (slot, found) = self.find(s, h);
        if found {
            let id = self.slots[slot] - 1;
            self.counts[id as usize] += 1;
            return id;
        }
        assert!(s.len() < 1 << LEN_BITS, "a pool string is under 16 MiB");
        let span = ((self.text.len() as u64) << LEN_BITS) | s.len() as u64;
        self.text.push_str(s);
        self.live += s.len();
        let id = match self.free.pop() {
            Some(id) => {
                let i = id as usize;
                (self.spans[i], self.counts[i], self.hashes[i]) = (span, 1, h);
                id
            }
            None => {
                self.spans.push(span);
                self.counts.push(1);
                self.hashes.push(h);
                (self.spans.len() - 1) as u32
            }
        };
        self.slots[slot] = id + 1;
        id
    }

    pub fn release(&mut self, id: u32) {
        if id == 0 {
            return;
        }
        let c = &mut self.counts[id as usize];
        *c -= 1;
        if *c != 0 {
            return;
        }
        let i = id as usize;
        let (slot, found) = self.find(self.get(id), self.hashes[i]);
        debug_assert!(found, "a counted string is in the table");
        self.remove_slot(slot);
        self.live -= self.span(id).1;
        self.spans[i] = 0;
        self.free.push(id);
        let dead = self.text.len() - self.live;
        if dead > self.live && dead > SLACK {
            self.pack();
        }
    }

    /// Empties `slot`, moving back the entries after it that probed past it.
    fn remove_slot(&mut self, mut hole: usize) {
        self.slots[hole] = 0;
        let mut i = (hole + 1) & self.mask();
        while self.slots[i] != 0 {
            let home = self.hashes[(self.slots[i] - 1) as usize] as usize & self.mask();
            // the entry may fill the hole unless its home lies after the hole, up to it
            if (i.wrapping_sub(home) & self.mask()) >= (i.wrapping_sub(hole) & self.mask()) {
                self.slots[hole] = self.slots[i];
                self.slots[i] = 0;
                hole = i;
            }
            i = (i + 1) & self.mask();
        }
    }

    fn grow(&mut self) {
        let len = (self.slots.len() * 2).max(16);
        self.slots = vec![0; len];
        for id in 1..self.spans.len() {
            if self.counts[id] != 0 {
                let mut i = self.hashes[id] as usize & self.mask();
                while self.slots[i] != 0 {
                    i = (i + 1) & self.mask();
                }
                self.slots[i] = id as u32 + 1;
            }
        }
    }

    /// The live strings end to end again, in id order.
    fn pack(&mut self) {
        let mut text = String::with_capacity(self.live);
        for (i, span) in self.spans.iter_mut().enumerate() {
            if self.counts[i] != 0 {
                let (start, len) = ((*span >> LEN_BITS) as usize, (*span & LEN_MASK) as usize);
                *span = ((text.len() as u64) << LEN_BITS) | len as u64;
                text.push_str(&self.text[start..start + len]);
            }
        }
        self.text = text;
    }

    /// Gives back what growing left unused, once a whole tree is in.
    pub fn trim(&mut self) {
        self.text.shrink_to_fit();
        self.spans.shrink_to_fit();
        self.counts.shrink_to_fit();
        self.hashes.shrink_to_fit();
    }

    fn span(&self, id: u32) -> (usize, usize) {
        let s = self.spans[id as usize];
        ((s >> LEN_BITS) as usize, (s & LEN_MASK) as usize)
    }

    pub fn get(&self, id: u32) -> &str {
        let (start, len) = self.span(id);
        &self.text[start..start + len]
    }

    /// The bytes held: the strings, dead ones not yet packed included,
    /// and the tables.
    pub fn bytes(&self) -> usize {
        self.text.capacity()
            + self.spans.capacity() * 8
            + (self.counts.capacity()
                + self.hashes.capacity()
                + self.slots.capacity()
                + self.free.capacity())
                * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_strings_share_an_id_until_the_last_release() {
        let mut p = Pool::new();
        let a = p.intern("home");
        let b = p.intern("home");
        assert_eq!(a, b);
        assert_eq!(p.intern(""), 0);
        p.release(a);
        assert_eq!(p.get(b), "home");
        p.release(b);
        let c = p.intern("other");
        assert_eq!(c, a, "the slot is used again");
        assert_eq!(p.get(c), "other");
    }

    /// Random interns and releases against a model of counted strings,
    /// long and short, so the table grows, entries shift back and the
    /// buffer is packed many times over.
    #[test]
    fn interns_and_releases_as_a_counted_map() {
        let mut p = Pool::new();
        let mut model: std::collections::HashMap<String, (u32, u32)> = Default::default();
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let mut next = |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        for step in 0..200_000 {
            let s = format!("{}{}", "s".repeat(next(3) as usize * 40), next(3000));
            if next(3) == 0 {
                let id = p.intern(&s);
                let e = model.entry(s.clone()).or_insert((id, 0));
                assert_eq!(e.0, id, "step {step}");
                e.1 += 1;
            } else if let Some((id, n)) = model.get_mut(&s) {
                p.release(*id);
                *n -= 1;
                if *n == 0 {
                    model.remove(&s);
                }
            }
            if step % 997 == 0 {
                for (s, (id, _)) in &model {
                    assert_eq!(p.get(*id), s);
                    let (_, found) = p.find(s, hash(p.seed, s));
                    assert!(found);
                }
                assert_eq!(p.live, model.keys().map(String::len).sum::<usize>());
                assert!(p.text.len() - p.live <= p.live.max(SLACK) + 200);
            }
        }
        let live: Vec<(String, u32)> = model.iter().map(|(s, (id, _))| (s.clone(), *id)).collect();
        for (s, id) in live {
            for _ in 0..model[&s].1 {
                p.release(id);
            }
        }
        assert_eq!(p.live, 0);
        assert!(p.slots.iter().all(|&e| e == 0));
    }

    #[test]
    fn strings_differing_in_their_last_byte_spread_out() {
        // seven bytes: the last one lands in the top half of the only word
        for seed in [0, 1, 0x1234_5678_9abc_def0, u64::MAX] {
            let mut starts = std::collections::HashMap::new();
            for b in 0..128u8 {
                let s = format!("abcdef{}", b as char);
                *starts.entry(hash(seed, &s) & 255).or_insert(0) += 1;
            }
            let most = starts.values().copied().max().unwrap();
            assert!(
                most <= 8,
                "seed {seed:x}: {most} strings share a start slot of 256"
            );
        }
    }

    #[test]
    fn colliding_strings_are_told_apart() {
        let mut p = Pool::new();
        // every string at one home slot: the table is a list probed in order
        let ids: Vec<u32> = (0..100).map(|i| p.intern(&format!("n{i}"))).collect();
        for h in p.hashes.iter_mut() {
            *h = 7;
        }
        let len = p.slots.len();
        p.slots = vec![0; len];
        p.grow();
        for (i, &id) in ids.iter().enumerate().step_by(3) {
            p.release(id);
            assert_eq!(p.counts[id as usize], 0, "{i}");
        }
        for (i, &id) in ids.iter().enumerate() {
            let s = format!("n{i}");
            let (slot, found) = p.find(&s, 7);
            assert_eq!(found, i % 3 != 0, "{s}");
            if found {
                assert_eq!(p.slots[slot], id + 1);
            }
        }
    }
}
