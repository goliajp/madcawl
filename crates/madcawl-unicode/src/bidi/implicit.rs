//! Isolating run sequences (X10) and the rules applied to each: W1 to W7, N0 to N2, I1 and I2.

use super::{Bidi, NONE};
use crate::BidiClass::{self, *};

fn isolate_initiator(c: BidiClass) -> bool {
    matches!(c, LRI | RLI | FSI)
}

fn dir(level: u8) -> BidiClass {
    if level & 1 == 1 { R } else { L }
}

// strong direction for N0 and N1: EN and AN count as R
pub(super) fn strong(c: BidiClass) -> Option<BidiClass> {
    match c {
        L => Some(L),
        R | AL | EN | AN => Some(R),
        _ => None,
    }
}

fn neutral(c: BidiClass) -> bool {
    matches!(c, B | S | WS | ON | LRI | RLI | FSI | PDI)
}

impl Bidi {
    pub(super) fn implicit(&mut self, start: usize, end: usize, para: u8) {
        self.runs.clear();
        for i in start..end {
            if self.removed[i] {
                continue;
            }
            match self.runs.last_mut() {
                Some(r) if self.levels[r.1 as usize] == self.levels[i] => r.1 = i as u32,
                _ => self.runs.push((i as u32, i as u32)),
            }
        }
        self.taken.clear();
        self.taken.resize(self.runs.len(), false);
        for r in 0..self.runs.len() {
            if self.taken[r] {
                continue;
            }
            self.seq.clear();
            let mut cur = r;
            loop {
                self.taken[cur] = true;
                let (a, b) = self.runs[cur];
                for i in a..=b {
                    if !self.removed[i as usize] {
                        self.seq.push(i);
                    }
                }
                let m = self.matching[b as usize];
                if !isolate_initiator(self.orig[b as usize]) || m == NONE {
                    break;
                }
                match self.runs.binary_search_by_key(&m, |r| r.0) {
                    Ok(next) => cur = next,
                    Err(_) => break,
                }
            }
            self.sequence(start, end, para);
        }
        self.implicit_levels(start, end);
    }

    fn sequence(&mut self, start: usize, end: usize, para: u8) {
        let seq = std::mem::take(&mut self.seq);
        let first = seq[0] as usize;
        let last = *seq.last().unwrap_or(&0) as usize;
        let level = self.levels[first];
        let before = (start..first)
            .rev()
            .find(|&i| !self.removed[i])
            .map_or(para, |i| self.levels[i]);
        let after = if isolate_initiator(self.orig[last]) {
            para
        } else {
            (last + 1..end)
                .find(|&i| !self.removed[i])
                .map_or(para, |i| self.levels[i])
        };
        let sos = dir(level.max(before));
        let eos = dir(level.max(after));
        self.weak(&seq, sos);
        self.brackets(&seq, sos, dir(level));
        self.neutrals(&seq, sos, eos, dir(level));
        self.seq = seq;
    }

    // I1, I2, after every sequence has read its neighbours' explicit levels
    fn implicit_levels(&mut self, start: usize, end: usize) {
        for i in start..end {
            if self.removed[i] {
                continue;
            }
            self.levels[i] += match (self.levels[i] & 1, self.types[i]) {
                (0, R) => 1,
                (0, AN | EN) => 2,
                (1, L | EN | AN) => 1,
                _ => 0,
            };
        }
    }

    fn weak(&mut self, seq: &[u32], sos: BidiClass) {
        let t = &mut self.types;
        // W1
        let mut prev = sos;
        for &i in seq {
            let i = i as usize;
            if t[i] == NSM {
                t[i] = if matches!(prev, LRI | RLI | FSI | PDI) {
                    ON
                } else {
                    prev
                };
            }
            prev = t[i];
        }
        // W2, W3
        let mut last_strong = sos;
        for &i in seq {
            let i = i as usize;
            match t[i] {
                L | R => last_strong = t[i],
                AL => {
                    last_strong = AL;
                    t[i] = R;
                }
                EN if last_strong == AL => t[i] = AN,
                _ => {}
            }
        }
        Self::numbers(t, seq);
        // W6, W7
        let mut last_strong = sos;
        for &i in seq {
            let i = i as usize;
            match t[i] {
                ES | ET | CS => t[i] = ON,
                L | R => last_strong = t[i],
                EN if last_strong == L => t[i] = L,
                _ => {}
            }
        }
    }

    // W4, W5
    fn numbers(t: &mut [BidiClass], seq: &[u32]) {
        for k in 1..seq.len().saturating_sub(1) {
            let (p, c, n) = (
                t[seq[k - 1] as usize],
                t[seq[k] as usize],
                t[seq[k + 1] as usize],
            );
            if (c == ES && p == EN && n == EN) || (c == CS && p == n && matches!(p, EN | AN)) {
                t[seq[k] as usize] = p;
            }
        }
        // W5
        let mut k = 0;
        while k < seq.len() {
            if t[seq[k] as usize] != ET {
                k += 1;
                continue;
            }
            let run = k;
            while k < seq.len() && t[seq[k] as usize] == ET {
                k += 1;
            }
            let touches = (run > 0 && t[seq[run - 1] as usize] == EN)
                || (k < seq.len() && t[seq[k] as usize] == EN);
            if touches {
                for &i in &seq[run..k] {
                    t[i as usize] = EN;
                }
            }
        }
    }

    // N1, N2
    fn neutrals(&mut self, seq: &[u32], sos: BidiClass, eos: BidiClass, e: BidiClass) {
        let t = &mut self.types;
        let mut k = 0;
        while k < seq.len() {
            if !neutral(t[seq[k] as usize]) {
                k += 1;
                continue;
            }
            let run = k;
            while k < seq.len() && neutral(t[seq[k] as usize]) {
                k += 1;
            }
            let before = if run == 0 {
                sos
            } else {
                strong(t[seq[run - 1] as usize]).unwrap_or(e)
            };
            let after = if k == seq.len() {
                eos
            } else {
                strong(t[seq[k] as usize]).unwrap_or(e)
            };
            let to = if before == after { before } else { e };
            for &i in &seq[run..k] {
                t[i as usize] = to;
            }
        }
    }
}
