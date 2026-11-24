//! Unicode character properties generated from the Unicode Character Database, and the algorithms built on them:
//! the bidirectional algorithm (UAX #9), line breaking (UAX #14) and grapheme cluster and word segmentation
//! (UAX #29).
//!
//! The algorithms read text through [`Source`], so the same code runs over UTF-8, UTF-16 and code point slices
//! without converting the text first.

mod bidi;
mod grapheme;
mod line;
mod source;
#[rustfmt::skip]
mod tables;
mod word;

pub use bidi::{Bidi, Direction, MAX_DEPTH};
pub use grapheme::{Graphemes, grapheme_breaks};
pub use line::{Break, LineBreaks};
pub use source::{Source, Utf16, Utf32};
pub use tables::{
    BidiClass, EastAsianWidth, GeneralCategory, GraphemeBreak, LineBreak, Script, UNICODE_VERSION,
    WordBreak,
};
pub use word::{WordBreaks, word_breaks};

/// Every property the tables hold for one code point.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Props {
    pub bidi: BidiClass,
    pub line: LineBreak,
    pub east_asian: EastAsianWidth,
    pub category: GeneralCategory,
    pub script: Script,
    pub grapheme: GraphemeBreak,
    pub word: WordBreak,
    // bit 0 Extended_Pictographic, bits 1-2 Indic_Conjunct_Break (none, linker, consonant, extend)
    flags: u8,
}

/// Indic_Conjunct_Break, used by grapheme cluster rule GB9c.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConjunctBreak {
    None,
    Linker,
    Consonant,
    Extend,
}

impl Props {
    pub fn extended_pictographic(&self) -> bool {
        self.flags & 1 != 0
    }

    pub fn conjunct_break(&self) -> ConjunctBreak {
        match self.flags >> 1 & 3 {
            0 => ConjunctBreak::None,
            1 => ConjunctBreak::Linker,
            2 => ConjunctBreak::Consonant,
            _ => ConjunctBreak::Extend,
        }
    }

    /// East_Asian_Width Fullwidth, Wide or Halfwidth: the `$EastAsian` set of UAX #14.
    pub fn east_asian_wide(&self) -> bool {
        matches!(
            self.east_asian,
            EastAsianWidth::F | EastAsianWidth::W | EastAsianWidth::H
        )
    }
}

/// The properties of a code point; values above U+10FFFF read as unassigned.
#[inline]
pub fn props(cp: u32) -> &'static Props {
    let cp = if cp < 0x11_0000 {
        cp as usize
    } else {
        0x10_FFFF
    };
    let i2 = tables::L1[cp >> 12] as usize * 64 + (cp >> 6 & 63);
    let i3 = tables::L2[i2] as usize * 64 + (cp & 63);
    &tables::RECORDS[tables::L3[i3] as usize]
}

/// Bidi_Mirroring_Glyph: the character to display in place of `cp` at an odd embedding level.
pub fn mirror(cp: u32) -> Option<u32> {
    tables::MIRRORS
        .binary_search_by_key(&cp, |m| m.0)
        .ok()
        .map(|i| tables::MIRRORS[i].1)
}

/// The paired bracket of `cp` if it is one: (the canonical opening bracket of its pair, whether it opens).
pub fn bracket(cp: u32) -> Option<(u32, bool)> {
    tables::BRACKETS
        .binary_search_by_key(&cp, |b| b.0)
        .ok()
        .map(|i| (tables::BRACKETS[i].1, tables::BRACKETS[i].2))
}

#[cfg(test)]
mod tests;
