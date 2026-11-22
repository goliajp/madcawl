//! Character references: `&amp;`, `&#38;`, `&#x26;`.

use super::{Go, REPLACEMENT, Sink, State, Step, Tokenizer, getc};
use crate::entities::{CHARS, NAMES};
use crate::error::ParseError;
use crate::input::push_char;

impl Tokenizer {
    /// Inside an attribute value, a reference goes into the value
    /// instead of being emitted.
    fn in_attribute(&self) -> bool {
        matches!(
            self.return_state,
            State::AttributeValueDoubleQuoted
                | State::AttributeValueSingleQuoted
                | State::AttributeValueUnquoted
        )
    }

    /// Flush the code points consumed as a character reference.
    fn flush_reference<S: Sink>(&mut self, sink: &mut S) {
        if self.in_attribute() {
            let temp = std::mem::take(&mut self.temp);
            self.attr_value().extend_from_slice(&temp);
            self.temp = temp;
        } else {
            Self::emit_bytes(sink, &self.temp);
        }
    }

    pub(super) fn character_reference<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.temp.clear();
        self.temp.push(b'&');
        let c = getc!(self, sink);
        match c {
            _ if is_alnum(c) => self.reconsume(State::NamedCharacterReference),
            0x23 => {
                self.temp.push(b'#');
                self.switch(State::NumericCharacterReference)
            }
            _ => {
                self.flush_reference(sink);
                let back = self.return_state;
                self.reconsume(back)
            }
        }
    }

    pub(super) fn named_character_reference<S: Sink>(&mut self, sink: &mut S) -> Go {
        let rest = self.input.rest();
        let Some((found, next)) = longest_match(rest, self.input.done()) else {
            return Go::Stop(Step::NeedInput);
        };
        let Some((index, len)) = found else {
            self.flush_reference(sink);
            return self.switch(State::AmbiguousAmpersand);
        };
        let name = NAMES[index];
        let ends_in_semicolon = name.last() == Some(&b';');
        if self.in_attribute()
            && !ends_in_semicolon
            && next.is_some_and(|b| b == b'=' || b.is_ascii_alphanumeric())
        {
            // left as written, for URLs like `?a=1&copy=2`
            self.temp.extend_from_slice(name);
            self.input.skip(len);
            self.flush_reference(sink);
            let back = self.return_state;
            return self.switch(back);
        }
        self.input.skip(len);
        if !ends_in_semicolon {
            sink.error(ParseError::MissingSemicolonAfterCharacterReference);
        }
        let (a, b) = CHARS[index];
        self.temp.clear();
        push_char(&mut self.temp, a);
        if b != 0 {
            push_char(&mut self.temp, b);
        }
        self.flush_reference(sink);
        let back = self.return_state;
        self.switch(back)
    }

    pub(super) fn ambiguous_ampersand<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        if is_alnum(c) {
            if self.in_attribute() {
                push_char(self.attr_value(), c);
            } else {
                self.emit_char(sink, c);
            }
            return Go::On;
        }
        if c == 0x3B {
            sink.error(ParseError::UnknownNamedCharacterReference);
        }
        let back = self.return_state;
        self.reconsume(back)
    }

    pub(super) fn numeric_character_reference<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.char_ref = 0;
        let c = getc!(self, sink);
        if c == 0x78 || c == 0x58 {
            self.temp.push(c as u8);
            return self.switch(State::HexadecimalCharacterReferenceStart);
        }
        self.reconsume(State::DecimalCharacterReferenceStart)
    }

    pub(super) fn numeric_start<S: Sink>(&mut self, sink: &mut S, radix: u32) -> Go {
        let c = getc!(self, sink);
        if digit(c, radix).is_some() {
            return self.reconsume(if radix == 16 {
                State::HexadecimalCharacterReference
            } else {
                State::DecimalCharacterReference
            });
        }
        sink.error(ParseError::AbsenceOfDigitsInNumericCharacterReference);
        self.flush_reference(sink);
        let back = self.return_state;
        self.reconsume(back)
    }

    pub(super) fn numeric_digits<S: Sink>(&mut self, sink: &mut S, radix: u32) -> Go {
        let c = getc!(self, sink);
        if let Some(d) = digit(c, radix) {
            // past the last code point it no longer matters by how much
            self.char_ref = self
                .char_ref
                .saturating_mul(radix)
                .saturating_add(d)
                .min(0x11_0000);
            return Go::On;
        }
        if c == 0x3B {
            return self.switch(State::NumericCharacterReferenceEnd);
        }
        sink.error(ParseError::MissingSemicolonAfterCharacterReference);
        self.reconsume(State::NumericCharacterReferenceEnd)
    }

    pub(super) fn numeric_end<S: Sink>(&mut self, sink: &mut S) -> Go {
        let mut c = self.char_ref;
        match c {
            0 => {
                sink.error(ParseError::NullCharacterReference);
                c = REPLACEMENT;
            }
            0x11_0000.. => {
                sink.error(ParseError::CharacterReferenceOutsideUnicodeRange);
                c = REPLACEMENT;
            }
            0xD800..=0xDFFF => {
                sink.error(ParseError::SurrogateCharacterReference);
                c = REPLACEMENT;
            }
            _ if (0xFDD0..=0xFDEF).contains(&c) || c & 0xFFFE == 0xFFFE => {
                sink.error(ParseError::NoncharacterCharacterReference);
            }
            0x0D | 0x01..=0x08 | 0x0B | 0x0E..=0x1F | 0x7F..=0x9F => {
                sink.error(ParseError::ControlCharacterReference);
                if let Some(&w) = WINDOWS_1252.get(c.wrapping_sub(0x80) as usize)
                    && w != 0
                {
                    c = w;
                }
            }
            _ => {}
        }
        self.temp.clear();
        push_char(&mut self.temp, c);
        self.flush_reference(sink);
        let back = self.return_state;
        self.switch(back)
    }
}

fn is_alnum(c: u32) -> bool {
    matches!(c, 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A)
}

fn digit(c: u32, radix: u32) -> Option<u32> {
    char::from_u32(c).and_then(|ch| ch.to_digit(radix))
}

/// An entity, by index into `NAMES`, and the length of its name.
type Entity = (usize, usize);

/// The longest entity name `rest` starts with, and the byte after it.
/// `None` when more input could make a longer match.
fn longest_match(rest: &[u8], done: bool) -> Option<(Option<Entity>, Option<u8>)> {
    let (mut lo, mut hi) = (0, NAMES.len());
    let mut best = None;
    let mut i = 0;
    if let Some(&b) = rest.first()
        && b < 0x80
    {
        let (a, z) = BY_FIRST[b as usize];
        if a == z {
            return Some((None, None));
        }
        (lo, hi) = (a as usize, z as usize);
        i = 1;
        if NAMES[lo].len() == 1 {
            best = Some((lo, 1));
        }
    }
    loop {
        let Some(&b) = rest.get(i) else {
            // every name still in range is longer than what has arrived
            if !done && lo < hi {
                return None;
            }
            break;
        };
        let range = &NAMES[lo..hi];
        let start = lo + range.partition_point(|n| n.get(i).is_none_or(|&x| x < b));
        let end = lo + range.partition_point(|n| n.get(i).is_none_or(|&x| x <= b));
        if start == end {
            break;
        }
        (lo, hi) = (start, end);
        i += 1;
        if NAMES[lo].len() == i {
            best = Some((lo, i));
        }
    }
    let next = best.and_then(|(_, len)| rest.get(len).copied());
    Some((best, next))
}

/// The range of `NAMES` that starts with each ASCII byte.
static BY_FIRST: [(u16, u16); 128] = {
    let mut out = [(0, 0); 128];
    let mut i = 0;
    while i < NAMES.len() {
        let b = NAMES[i][0] as usize;
        if out[b].0 == out[b].1 {
            out[b].0 = i as u16;
        }
        out[b].1 = i as u16 + 1;
        i += 1;
    }
    out
};

/// What a numeric reference to U+0080..U+009F means: what windows-1252
/// puts there. Zero where it has nothing.
static WINDOWS_1252: [u32; 32] = [
    0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0, 0x017D, 0, 0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC,
    0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
];
