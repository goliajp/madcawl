//! The names and values records point at: each distinct string stored
//! once, counted, and freed when no record uses it.

use std::collections::HashMap;
use std::rc::Rc;

pub(crate) struct Pool {
    /// What a freed entry holds, shared
    empty: Rc<str>,
    strings: Vec<Rc<str>>,
    counts: Vec<u32>,
    map: HashMap<Rc<str>, u32>,
    free: Vec<u32>,
    bytes: usize,
}

impl Pool {
    pub fn new() -> Self {
        let empty: Rc<str> = Rc::from("");
        Pool {
            strings: vec![empty.clone()],
            empty,
            counts: vec![0],
            map: HashMap::new(),
            free: Vec::new(),
            bytes: 0,
        }
    }

    /// The id of `s`, counted once more; 0 is the empty string.
    pub fn intern(&mut self, s: &str) -> u32 {
        if s.is_empty() {
            return 0;
        }
        if let Some(&id) = self.map.get(s) {
            self.counts[id as usize] += 1;
            return id;
        }
        let rc: Rc<str> = Rc::from(s);
        self.bytes += s.len();
        let id = match self.free.pop() {
            Some(id) => {
                self.strings[id as usize] = rc.clone();
                self.counts[id as usize] = 1;
                id
            }
            None => {
                self.strings.push(rc.clone());
                self.counts.push(1);
                (self.strings.len() - 1) as u32
            }
        };
        self.map.insert(rc, id);
        id
    }

    pub fn release(&mut self, id: u32) {
        if id == 0 {
            return;
        }
        let c = &mut self.counts[id as usize];
        *c -= 1;
        if *c == 0 {
            let s = std::mem::replace(&mut self.strings[id as usize], self.empty.clone());
            self.bytes -= s.len();
            self.map.remove(&s);
            self.free.push(id);
        }
    }

    pub fn get(&self, id: u32) -> &str {
        &self.strings[id as usize]
    }

    /// The bytes held: the strings, their table slots and map entries.
    pub fn bytes(&self) -> usize {
        self.bytes
            + self.strings.capacity() * 16
            + self.counts.capacity() * 4
            + self.map.capacity() * 24
            + self.free.capacity() * 4
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
}
