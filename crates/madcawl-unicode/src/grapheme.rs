//! Extended grapheme cluster boundaries (UAX #29 section 3).

use crate::{ConjunctBreak, GraphemeBreak as G, Props, Source, props};

/// The boundaries of the extended grapheme clusters of a text: every offset after the start where a cluster
/// begins, then the end of the text. An empty text has none.
pub struct Graphemes<'a, S: Source + ?Sized> {
    src: &'a S,
    pos: usize,
    prev: Option<&'static Props>,
    // an odd number of regional indicators ends at the previous character
    ri_odd: bool,
    // 1 after Extended_Pictographic Extend*, 2 after that and a ZWJ
    emoji: u8,
    // InCB=Linker InCB=Extend* ends at the previous character
    linker: bool,
}

pub fn grapheme_breaks<S: Source + ?Sized>(src: &S) -> Graphemes<'_, S> {
    Graphemes {
        src,
        pos: 0,
        prev: None,
        ri_odd: false,
        emoji: 0,
        linker: false,
    }
}

impl<S: Source + ?Sized> Graphemes<'_, S> {
    fn joins(&self, a: &Props, b: &Props) -> bool {
        match (a.grapheme, b.grapheme) {
            (G::CR, G::LF) => true,
            (G::Control | G::CR | G::LF, _) | (_, G::Control | G::CR | G::LF) => false,
            (G::L, G::L | G::V | G::LV | G::LVT)
            | (G::LV | G::V, G::V | G::T)
            | (G::LVT | G::T, G::T) => true,
            (_, G::Extend | G::ZWJ | G::SpacingMark) | (G::Prepend, _) => true,
            _ if self.linker && b.conjunct_break() == ConjunctBreak::Consonant => true,
            _ if self.emoji == 2 && b.extended_pictographic() => true,
            (G::RegionalIndicator, G::RegionalIndicator) => self.ri_odd,
            _ => false,
        }
    }

    fn take(&mut self, p: &'static Props) {
        self.ri_odd = p.grapheme == G::RegionalIndicator && !self.ri_odd;
        self.emoji = match (p.grapheme, self.emoji) {
            _ if p.extended_pictographic() => 1,
            (G::Extend, 1) => 1,
            (G::ZWJ, 1) => 2,
            _ => 0,
        };
        self.linker = match p.conjunct_break() {
            ConjunctBreak::Linker => true,
            ConjunctBreak::Extend => self.linker,
            _ => false,
        };
        self.prev = Some(p);
    }
}

impl<S: Source + ?Sized> Iterator for Graphemes<'_, S> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        let len = self.src.len();
        loop {
            if self.pos >= len {
                return self.prev.take().map(|_| len);
            }
            let at = self.pos;
            let (cp, end) = self.src.next(at);
            let p = props(cp);
            self.pos = end;
            let brk = match self.prev {
                Some(a) => !self.joins(a, p),
                None => false,
            };
            self.take(p);
            if brk {
                return Some(at);
            }
        }
    }
}
