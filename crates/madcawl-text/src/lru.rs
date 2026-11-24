//! A bounded map that evicts its least recently used entries one at a time.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hash, Hasher};

const NIL: u32 = u32::MAX;

/// A multiply-rotate hasher for small keys and text; not resistant to collisions. Maps keyed by its 64-bit
/// hash alone must store the full key and compare it on every hit.
#[derive(Default)]
pub(crate) struct Fx(u64);

impl Hasher for Fx {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut v = [0u8; 8];
            v[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(v));
        }
    }

    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn write_u32(&mut self, v: u32) {
        self.write_u64(v as u64);
    }

    fn write_u16(&mut self, v: u16) {
        self.write_u64(v as u64);
    }

    fn write_u8(&mut self, v: u8) {
        self.write_u64(v as u64);
    }

    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

pub(crate) type FxBuild = BuildHasherDefault<Fx>;

struct Slot<K, V> {
    key: K,
    value: V,
    weight: usize,
    prev: u32,
    next: u32,
}

/// At most `cap` entries and at most `max_weight` total weight; inserting past either bound evicts from the
/// least recently used end until both hold again. An entry heavier than `max_weight / 16` is not kept.
pub(crate) struct Lru<K, V> {
    map: HashMap<K, u32, FxBuild>,
    slots: Vec<Option<Slot<K, V>>>,
    free: Vec<u32>,
    // most and least recently used
    head: u32,
    tail: u32,
    cap: usize,
    weight: usize,
    max_weight: usize,
    pub(crate) evictions: u64,
}

impl<K: Hash + Eq + Clone, V> Lru<K, V> {
    pub(crate) fn new(cap: usize, max_weight: usize) -> Self {
        Lru {
            map: HashMap::default(),
            slots: Vec::new(),
            free: Vec::new(),
            head: NIL,
            tail: NIL,
            cap: cap.max(1),
            weight: 0,
            max_weight,
            evictions: 0,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }

    pub(crate) fn weight(&self) -> usize {
        self.weight
    }

    fn slot(&mut self, i: u32) -> &mut Slot<K, V> {
        match &mut self.slots[i as usize] {
            Some(s) => s,
            None => unreachable!("linked slot is empty"),
        }
    }

    fn unlink(&mut self, i: u32) {
        let (prev, next) = {
            let s = self.slot(i);
            (s.prev, s.next)
        };
        if prev == NIL {
            self.head = next
        } else {
            self.slot(prev).next = next
        }
        if next == NIL {
            self.tail = prev
        } else {
            self.slot(next).prev = prev
        }
    }

    fn push_front(&mut self, i: u32) {
        let head = self.head;
        {
            let s = self.slot(i);
            s.prev = NIL;
            s.next = head;
        }
        if head == NIL {
            self.tail = i
        } else {
            self.slot(head).prev = i
        }
        self.head = i;
    }

    /// The value for `key`, marked most recently used.
    pub(crate) fn get(&mut self, key: &K) -> Option<&V> {
        let i = *self.map.get(key)?;
        if self.head != i {
            self.unlink(i);
            self.push_front(i);
        }
        self.slots[i as usize].as_ref().map(|s| &s.value)
    }

    pub(crate) fn insert(&mut self, key: K, value: V, weight: usize) {
        if let Some(&i) = self.map.get(&key) {
            self.unlink(i);
            self.weight -= self.slot(i).weight;
            self.slots[i as usize] = None;
            self.free.push(i);
            self.map.remove(&key);
        }
        // one entry may take at most a sixteenth of the weight, so a huge one cannot flush the cache
        if weight > self.max_weight / 16 {
            return;
        }
        while self.tail != NIL
            && (self.map.len() >= self.cap || self.weight + weight > self.max_weight)
        {
            self.evict();
        }
        let slot = Slot {
            key: key.clone(),
            value,
            weight,
            prev: NIL,
            next: NIL,
        };
        let i = match self.free.pop() {
            Some(i) => {
                self.slots[i as usize] = Some(slot);
                i
            }
            None => {
                self.slots.push(Some(slot));
                (self.slots.len() - 1) as u32
            }
        };
        self.push_front(i);
        self.map.insert(key, i);
        self.weight += weight;
    }

    fn evict(&mut self) {
        let i = self.tail;
        self.unlink(i);
        if let Some(s) = self.slots[i as usize].take() {
            self.weight -= s.weight;
            self.map.remove(&s.key);
        }
        self.free.push(i);
        self.evictions += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::Lru;

    #[test]
    fn an_entry_above_the_per_entry_cap_is_not_cached() {
        let mut lru = Lru::new(100, 1600);
        for k in 0..10u32 {
            lru.insert(k, k, 50);
        }
        lru.insert(99, 99, 1000);
        assert_eq!(lru.get(&99), None);
        assert_eq!((lru.len(), lru.weight(), lru.evictions), (10, 500, 0));
        // replacing a key with a value too heavy to keep drops the old value
        lru.insert(3, 33, 1000);
        assert_eq!(lru.get(&3), None);
        assert_eq!((lru.len(), lru.weight()), (9, 450));
        lru.insert(10, 10, 100);
        assert_eq!(lru.get(&10), Some(&10));
    }
}
