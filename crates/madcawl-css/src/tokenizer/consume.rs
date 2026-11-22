//! The longer tokens: numbers, names and functions, urls, strings and
//! unicode ranges.

use super::{AT_EOF, ESCAPED, INTEGER, Kind, Token, Tokenizer};
use crate::Span;
use crate::escape::{
    EOF, hex_value, is_digit, is_hex, is_newline, is_non_printable, is_whitespace,
};

impl Tokenizer<'_> {
    pub(super) fn numeric(&mut self, start: usize) -> Token {
        let mut i = start;
        let mut flags = INTEGER;
        if matches!(self.at(i), 0x2B | 0x2D) {
            i += 1;
        }
        while is_digit(self.at(i)) {
            i += 1;
        }
        if self.at(i) == b'.' as u16 && is_digit(self.at(i + 1)) {
            flags = 0;
            i += 1;
            while is_digit(self.at(i)) {
                i += 1;
            }
        }
        if matches!(self.at(i), 0x45 | 0x65) {
            let n = self.at(i + 1);
            let signed = matches!(n, 0x2B | 0x2D) && is_digit(self.at(i + 2));
            if is_digit(n) || signed {
                flags = 0;
                i += if signed { 2 } else { 1 };
                while is_digit(self.at(i)) {
                    i += 1;
                }
            }
        }
        let repr_end = i;
        self.pos = i;
        let kind = if self.starts_ident(i) {
            if self.ident_sequence() {
                flags |= ESCAPED;
            }
            Kind::Dimension
        } else if self.at(i) == b'%' as u16 {
            self.pos += 1;
            Kind::Percentage
        } else {
            Kind::Number
        };
        Token {
            kind,
            span: Span::new(start, self.pos),
            value: Span::new(start, repr_end),
            unit_start: if kind == Kind::Dimension {
                repr_end as u32
            } else {
                self.pos as u32
            },
            flags,
        }
    }

    pub(super) fn ident_like(&mut self, start: usize) -> Token {
        self.pos = start;
        let escaped = self.ident_sequence();
        let name = Span::new(start, self.pos);
        let flags = if escaped { ESCAPED } else { 0 };
        if self.at(self.pos) != b'(' as u16 {
            return self.make_named(Kind::Ident, start, name, flags);
        }
        let probe = Token {
            kind: Kind::Ident,
            span: name,
            value: name,
            unit_start: name.end,
            flags,
        };
        self.pos += 1;
        if !probe.value_is(self.src, "url") {
            return self.make_named(Kind::Function, start, name, flags);
        }
        while is_whitespace(self.at(self.pos)) && is_whitespace(self.at(self.pos + 1)) {
            self.pos += 1;
        }
        let q = |c: u16| c == 0x22 || c == 0x27;
        let c = self.at(self.pos);
        if q(c) || (is_whitespace(c) && q(self.at(self.pos + 1))) {
            return self.make_named(Kind::Function, start, name, flags);
        }
        self.url(start)
    }

    fn make_named(&self, kind: Kind, start: usize, name: Span, flags: u8) -> Token {
        Token {
            kind,
            span: Span::new(start, self.pos),
            value: name,
            unit_start: self.pos as u32,
            flags,
        }
    }

    fn url(&mut self, start: usize) -> Token {
        while is_whitespace(self.at(self.pos)) {
            self.pos += 1;
        }
        let vstart = self.pos;
        let mut flags = 0;
        let named = |t: &Self, kind, vend: usize, flags| Token {
            kind,
            span: Span::new(start, t.pos),
            value: Span::new(vstart, vend),
            unit_start: t.pos as u32,
            flags,
        };
        loop {
            let c = self.at(self.pos);
            match c {
                EOF => return named(self, Kind::Url, self.pos, flags | AT_EOF),
                0x29 => {
                    self.pos += 1;
                    return named(self, Kind::Url, self.pos - 1, flags);
                }
                _ if is_whitespace(c) => {
                    let vend = self.pos;
                    while is_whitespace(self.at(self.pos)) {
                        self.pos += 1;
                    }
                    match self.at(self.pos) {
                        0x29 => {
                            self.pos += 1;
                            return named(self, Kind::Url, vend, flags);
                        }
                        EOF => return named(self, Kind::Url, vend, flags | AT_EOF),
                        _ => return self.bad_url(start),
                    }
                }
                0x22 | 0x27 | 0x28 => return self.bad_url(start),
                _ if is_non_printable(c) => return self.bad_url(start),
                0x5C => {
                    if !self.valid_escape(self.pos) {
                        return self.bad_url(start);
                    }
                    flags |= ESCAPED;
                    self.pos += 1;
                    self.skip_escape();
                }
                _ => {
                    flags |= if c == 0 { ESCAPED } else { 0 };
                    self.pos += 1;
                }
            }
        }
    }

    fn bad_url(&mut self, start: usize) -> Token {
        loop {
            match self.at(self.pos) {
                EOF => break,
                0x29 => {
                    self.pos += 1;
                    break;
                }
                _ if self.valid_escape(self.pos) => {
                    self.pos += 1;
                    self.skip_escape();
                }
                _ => self.pos += 1,
            }
        }
        self.make(Kind::BadUrl, start, start, 0)
    }

    pub(super) fn string(&mut self, start: usize, quote: u16) -> Token {
        self.pos = start + 1;
        let mut flags = 0;
        let done = |t: &Self, kind, vend: usize, flags| Token {
            kind,
            span: Span::new(start, t.pos),
            value: Span::new(start + 1, vend),
            unit_start: t.pos as u32,
            flags,
        };
        loop {
            let c = self.at(self.pos);
            if c == EOF {
                return done(self, Kind::String, self.pos, flags | AT_EOF);
            }
            if c == quote {
                self.pos += 1;
                return done(self, Kind::String, self.pos - 1, flags);
            }
            if is_newline(c) {
                return done(self, Kind::BadString, self.pos, flags);
            }
            if c == b'\\' as u16 {
                flags |= ESCAPED;
                self.pos += 1;
                let n = self.at(self.pos);
                if is_newline(n) {
                    self.skip_one_whitespace();
                } else {
                    self.skip_escape();
                }
                continue;
            }
            flags |= if c == 0 { ESCAPED } else { 0 };
            self.pos += 1;
        }
    }

    pub(super) fn unicode_range(&mut self, start: usize) -> Token {
        let mut i = start + 2;
        let (mut lo, mut hi, mut n) = (0u32, 0u32, 0);
        while n < 6 && is_hex(self.at(i)) {
            let d = hex_value(self.s[i]);
            lo = lo * 16 + d;
            hi = hi * 16 + d;
            i += 1;
            n += 1;
        }
        let mut wild = false;
        while n < 6 && self.at(i) == b'?' as u16 {
            lo *= 16;
            hi = hi * 16 + 15;
            i += 1;
            n += 1;
            wild = true;
        }
        if !wild && self.at(i) == b'-' as u16 && is_hex(self.at(i + 1)) {
            i += 1;
            hi = 0;
            let mut k = 0;
            while k < 6 && is_hex(self.at(i)) {
                hi = hi * 16 + hex_value(self.s[i]);
                i += 1;
                k += 1;
            }
        }
        self.pos = i;
        self.make(Kind::UnicodeRange { start: lo, end: hi }, start, start, 0)
    }
}
