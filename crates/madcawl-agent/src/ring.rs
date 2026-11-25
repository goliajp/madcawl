//! One agent session's record of changes: a bounded ring of (sequence,
//! slot, generation, delta). A diff from sequence N reads the entries
//! after N; once entries after N have been dropped, the session is too
//! far behind and takes a snapshot instead.

use std::collections::VecDeque;

use crate::sync::Delta;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Entry {
    pub seq: u64,
    pub slot: u32,
    pub generation: u32,
    pub delta: Delta,
}

pub(crate) struct Ring {
    entries: VecDeque<Entry>,
    cap: usize,
    /// Every change after this sequence is still in the ring
    complete_after: u64,
}

impl Ring {
    pub fn new(cap: usize, now: u64) -> Self {
        Ring {
            entries: VecDeque::new(),
            cap,
            complete_after: now,
        }
    }

    pub fn push(&mut self, e: Entry) {
        if self.entries.len() == self.cap {
            let dropped = self.entries.pop_front().expect("full");
            self.complete_after = self.complete_after.max(dropped.seq);
        }
        self.entries.push_back(e);
    }

    /// The entries after `since`, or `None` when some were dropped.
    pub fn since(&self, since: u64) -> Option<impl Iterator<Item = &Entry>> {
        (since >= self.complete_after).then(|| self.entries.iter().filter(move |e| e.seq > since))
    }

    pub fn bytes(&self) -> usize {
        self.entries.capacity() * size_of::<Entry>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_past_what_the_ring_dropped_is_refused() {
        let mut r = Ring::new(2, 0);
        for seq in 1..=3 {
            r.push(Entry {
                seq,
                slot: seq as u32,
                generation: 0,
                delta: Delta::Changed,
            });
        }
        assert!(r.since(0).is_none(), "the change of 1 was dropped");
        let got: Vec<u32> = r.since(1).unwrap().map(|e| e.slot).collect();
        assert_eq!(got, [2, 3]);
        assert_eq!(r.since(3).unwrap().count(), 0);
    }
}
