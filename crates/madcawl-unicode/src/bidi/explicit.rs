//! Explicit levels and directions: rules X1 to X9.

use super::{Bidi, MAX_DEPTH, NONE};
use crate::BidiClass::{self, *};

#[derive(Clone, Copy)]
struct Entry {
    level: u8,
    // the class an override forces, if any
    force: Option<BidiClass>,
    isolate: bool,
}

// the directional status stack (X1: max_depth + 2 entries) and the three counters
struct Status {
    stack: [Entry; MAX_DEPTH as usize + 2],
    depth: usize,
    over_isolate: u32,
    over_embed: u32,
    valid_isolate: u32,
}

fn next_level(level: u8, rtl: bool) -> u8 {
    if rtl {
        (level + 1) | 1
    } else {
        (level + 2) & !1
    }
}

impl Status {
    fn new(para: u8) -> Self {
        let entry = Entry {
            level: para,
            force: None,
            isolate: false,
        };
        Status {
            stack: [entry; MAX_DEPTH as usize + 2],
            depth: 1,
            over_isolate: 0,
            over_embed: 0,
            valid_isolate: 0,
        }
    }

    fn top(&self) -> Entry {
        self.stack[self.depth - 1]
    }

    fn push(&mut self, entry: Entry) {
        self.stack[self.depth] = entry;
        self.depth += 1;
    }

    // X2 to X5
    fn embed(&mut self, c: BidiClass) {
        let level = next_level(self.top().level, matches!(c, RLE | RLO));
        if level <= MAX_DEPTH && self.over_isolate == 0 && self.over_embed == 0 {
            let force = match c {
                RLO => Some(R),
                LRO => Some(L),
                _ => None,
            };
            self.push(Entry {
                level,
                force,
                isolate: false,
            });
        } else if self.over_isolate == 0 {
            self.over_embed += 1;
        }
    }

    // X5a to X5c, after the initiator took the current level
    fn isolate(&mut self, rtl: bool) {
        let level = next_level(self.top().level, rtl);
        if level <= MAX_DEPTH && self.over_isolate == 0 && self.over_embed == 0 {
            self.valid_isolate += 1;
            self.push(Entry {
                level,
                force: None,
                isolate: true,
            });
        } else {
            self.over_isolate += 1;
        }
    }

    // X6a, before the PDI takes the current level
    fn pop_isolate(&mut self) {
        if self.over_isolate > 0 {
            self.over_isolate -= 1;
        } else if self.valid_isolate > 0 {
            self.over_embed = 0;
            while !self.top().isolate {
                self.depth -= 1;
            }
            self.depth -= 1;
            self.valid_isolate -= 1;
        }
    }

    // X7
    fn pop_embedding(&mut self) {
        if self.over_isolate > 0 {
            return;
        }
        if self.over_embed > 0 {
            self.over_embed -= 1;
        } else if !self.top().isolate && self.depth >= 2 {
            self.depth -= 1;
        }
    }
}

impl Bidi {
    // X6: the current level, and the class an override forces
    fn take_level(&mut self, i: usize, top: Entry) {
        self.levels[i] = top.level;
        if let Some(f) = top.force {
            self.types[i] = f;
        }
    }

    pub(super) fn explicit(&mut self, start: usize, end: usize, para: u8) {
        let mut st = Status::new(para);
        for i in start..end {
            let top = st.top();
            match self.orig[i] {
                c @ (RLE | LRE | RLO | LRO | PDF | BN) => {
                    match c {
                        PDF => st.pop_embedding(),
                        BN => {}
                        _ => st.embed(c),
                    }
                    // X9
                    self.removed[i] = true;
                    self.levels[i] = top.level;
                }
                c @ (RLI | LRI | FSI) => {
                    self.take_level(i, top);
                    let rtl = match c {
                        RLI => true,
                        LRI => false,
                        _ => {
                            let stop = match self.matching[i] {
                                NONE => end,
                                m => m as usize,
                            };
                            self.first_strong(i + 1, stop) == Some(1)
                        }
                    };
                    st.isolate(rtl);
                }
                PDI => {
                    st.pop_isolate();
                    self.take_level(i, st.top());
                }
                B => self.levels[i] = para,
                _ => self.take_level(i, top),
            }
        }
    }
}
