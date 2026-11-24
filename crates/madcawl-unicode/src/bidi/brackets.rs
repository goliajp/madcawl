//! Paired brackets: BD16 and rule N0.

use super::implicit::strong;
use super::{Bidi, NONE, OPENS};
use crate::BidiClass::{self, *};

const STACK: usize = 63;

impl Bidi {
    // BD16 into `self.pairs`, as positions in `seq`
    fn find_pairs(&mut self, seq: &[u32]) {
        self.pairs.clear();
        let mut stack = [(0u32, 0u32); STACK];
        let mut depth = 0;
        for (k, &i) in seq.iter().enumerate() {
            let i = i as usize;
            let p = self.pair[i];
            if p == NONE || self.types[i] != ON {
                continue;
            }
            let open = p & !OPENS;
            if p & OPENS != 0 {
                if depth == STACK {
                    self.pairs.clear();
                    return;
                }
                stack[depth] = (open, k as u32);
                depth += 1;
            } else if let Some(d) = (0..depth).rev().find(|&d| stack[d].0 == open) {
                self.pairs.push((stack[d].1, k as u32));
                depth = d;
            }
        }
        self.pairs.sort_unstable();
    }

    pub(super) fn brackets(&mut self, seq: &[u32], sos: BidiClass, e: BidiClass) {
        self.find_pairs(seq);
        for n in 0..self.pairs.len() {
            let (o, c) = self.pairs[n];
            let (o, c) = (o as usize, c as usize);
            let mut opposite = false;
            let mut matched = false;
            for &i in &seq[o + 1..c] {
                match strong(self.types[i as usize]) {
                    Some(d) if d == e => {
                        matched = true;
                        break;
                    }
                    Some(_) => opposite = true,
                    None => {}
                }
            }
            let to = if matched {
                e
            } else if opposite {
                let context = seq[..o]
                    .iter()
                    .rev()
                    .find_map(|&i| strong(self.types[i as usize]))
                    .unwrap_or(sos);
                if context != e { context } else { e }
            } else {
                continue;
            };
            for k in [o, c] {
                self.types[seq[k] as usize] = to;
                for &i in &seq[k + 1..] {
                    if self.orig[i as usize] != NSM {
                        break;
                    }
                    self.types[i as usize] = to;
                }
            }
        }
    }
}
