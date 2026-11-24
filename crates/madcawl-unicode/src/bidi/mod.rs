//! The Unicode bidirectional algorithm (UAX #9): paragraph levels, explicit and implicit embedding levels, and
//! the reordering of a line.

mod brackets;
mod explicit;
mod implicit;
mod line;

use crate::{BidiClass, Source, bracket, props};
use std::ops::Range;

/// The deepest explicit embedding level (BD2).
pub const MAX_DEPTH: u8 = 125;

const NONE: u32 = u32::MAX;
// set in `pair` for an opening bracket; the rest is the canonical opening bracket of the pair
const OPENS: u32 = 1 << 31;
// buffers above this many elements are shrunk back to it after a text, so one long text does not pin memory
pub(crate) const KEEP: usize = 1 << 16;

/// The base direction of a paragraph: given, or found from its first strong character (P2, P3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Ltr,
    Rtl,
    Auto,
}

/// A paragraph found by rule P1: code point indices and its embedding level.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Paragraph {
    pub range: Range<usize>,
    pub level: u8,
}

/// The bidirectional algorithm run over one text. The buffers are kept between texts, up to a bound.
#[derive(Default)]
pub struct Bidi {
    // per code point
    pub(crate) orig: Vec<BidiClass>,
    pub(crate) types: Vec<BidiClass>,
    pub(crate) levels: Vec<u8>,
    pub(crate) removed: Vec<bool>,
    pub(crate) pair: Vec<u32>,
    pub(crate) matching: Vec<u32>,
    offsets: Vec<usize>,
    paragraphs: Vec<Paragraph>,
    // scratch
    pub(crate) runs: Vec<(u32, u32)>,
    pub(crate) seq: Vec<u32>,
    pub(crate) stack: Vec<u32>,
    pub(crate) pairs: Vec<(u32, u32)>,
    pub(crate) taken: Vec<bool>,
}

impl Bidi {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves the embedding levels of every paragraph of `src`.
    pub fn resolve<S: Source + ?Sized>(&mut self, src: &S, dir: Direction) {
        self.clear();
        let mut i = 0;
        while i < src.len() {
            let (cp, next) = src.next(i);
            self.orig.push(props(cp).bidi);
            self.pair.push(match bracket(cp) {
                Some((open, opens)) => open | if opens { OPENS } else { 0 },
                None => NONE,
            });
            self.offsets.push(i);
            i = next;
        }
        self.offsets.push(src.len());
        self.run(dir);
    }

    /// Resolves levels for a sequence of bidi classes with no paired brackets, as the class-only conformance
    /// data specifies them.
    pub fn resolve_classes(&mut self, classes: &[BidiClass], dir: Direction) {
        self.clear();
        self.orig.extend_from_slice(classes);
        self.pair.resize(classes.len(), NONE);
        self.offsets.extend(0..=classes.len());
        self.run(dir);
    }

    fn clear(&mut self) {
        self.orig.clear();
        self.pair.clear();
        self.offsets.clear();
        self.paragraphs.clear();
    }

    fn run(&mut self, dir: Direction) {
        let n = self.orig.len();
        self.types.clear();
        self.types.extend_from_slice(&self.orig);
        self.levels.clear();
        self.levels.resize(n, 0);
        self.removed.clear();
        self.removed.resize(n, false);
        self.matching.clear();
        self.matching.resize(n, NONE);
        let mut start = 0;
        while start < n {
            let mut end = start;
            while end < n && self.orig[end] != BidiClass::B {
                end += 1;
            }
            let end = (end + 1).min(n);
            self.match_isolates(start, end);
            let level = match dir {
                Direction::Ltr => 0,
                Direction::Rtl => 1,
                Direction::Auto => self.first_strong(start, end).unwrap_or(0),
            };
            self.paragraphs.push(Paragraph {
                range: start..end,
                level,
            });
            self.explicit(start, end, level);
            self.implicit(start, end, level);
            self.fill_removed(start, end, level);
            start = end;
        }
        self.shrink();
    }

    fn shrink(&mut self) {
        self.orig.shrink_to(KEEP);
        self.types.shrink_to(KEEP);
        self.levels.shrink_to(KEEP);
        self.removed.shrink_to(KEEP);
        self.taken.shrink_to(KEEP);
        for v in [
            &mut self.pair,
            &mut self.matching,
            &mut self.seq,
            &mut self.stack,
        ] {
            v.shrink_to(KEEP);
        }
        self.offsets.shrink_to(KEEP);
        self.paragraphs.shrink_to(KEEP);
        self.runs.shrink_to(KEEP);
        self.pairs.shrink_to(KEEP);
    }

    // BD9: each isolate initiator's matching PDI
    fn match_isolates(&mut self, start: usize, end: usize) {
        use BidiClass::*;
        self.stack.clear();
        for i in start..end {
            match self.orig[i] {
                LRI | RLI | FSI => self.stack.push(i as u32),
                PDI => {
                    if let Some(j) = self.stack.pop() {
                        self.matching[j as usize] = i as u32;
                        self.matching[i] = j;
                    }
                }
                _ => {}
            }
        }
    }

    // P2, P3: the level of the first strong character, skipping isolates; `None` when there is none
    pub(crate) fn first_strong(&self, start: usize, end: usize) -> Option<u8> {
        use BidiClass::*;
        let mut i = start;
        while i < end {
            match self.orig[i] {
                L => return Some(0),
                R | AL => return Some(1),
                LRI | RLI | FSI => match self.matching[i] {
                    NONE => return None,
                    m => i = m as usize,
                },
                B => return None,
                _ => {}
            }
            i += 1;
        }
        None
    }

    // characters removed by X9 take the level of the character before them, for display
    fn fill_removed(&mut self, start: usize, end: usize, para: u8) {
        let mut last = para;
        for i in start..end {
            if self.removed[i] {
                self.levels[i] = last;
            } else {
                last = self.levels[i];
            }
        }
    }

    /// The paragraphs of the last text resolved.
    pub fn paragraphs(&self) -> &[Paragraph] {
        &self.paragraphs
    }

    /// The resolved embedding level of each code point, before the line-based rule L1.
    pub fn levels(&self) -> &[u8] {
        &self.levels
    }

    /// Whether rule X9 removed the code point at `i` (embedding controls and boundary neutrals); its level is
    /// then the level of the character before it.
    pub fn is_removed(&self, i: usize) -> bool {
        self.removed[i]
    }

    /// The code unit offset in the source of code point `i`; `i == len` gives the source length.
    pub fn offset(&self, i: usize) -> usize {
        self.offsets[i]
    }

    /// The number of code points resolved.
    pub fn len(&self) -> usize {
        self.orig.len()
    }

    pub fn is_empty(&self) -> bool {
        self.orig.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        [
            self.orig.capacity(),
            self.types.capacity(),
            self.levels.capacity(),
            self.removed.capacity(),
            self.pair.capacity(),
            self.matching.capacity(),
            self.offsets.capacity(),
            self.paragraphs.capacity(),
            self.runs.capacity(),
            self.seq.capacity(),
            self.stack.capacity(),
            self.pairs.capacity(),
            self.taken.capacity(),
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
    }
}
