//! The tokenizer of CSS Syntax Level 3, pulled one token at a time over
//! UTF-8 source. A token is a few spans of the source and some flags;
//! nothing is copied or allocated. The input preprocessing (CR LF and
//! FF as newlines, NUL as U+FFFD) happens where it matters: newlines
//! are matched as any of the three, and a NUL marks the value as one to
//! decode (`Token::chars`).
//!
//! A tokenizer can stop at any position short of the end of the source,
//! and then sees the input end there: a rule's prelude or body is
//! tokenized again from its span with the positions it has in the
//! whole source.

mod consume;
mod token;

pub use token::{AT_EOF, ESCAPED, INTEGER, Kind, Token};

use crate::Span;
use crate::escape::{
    EOF, is_digit, is_hex, is_ident_char, is_ident_start, is_newline, is_whitespace,
};

#[derive(Clone)]
pub struct Tokenizer<'a> {
    src: &'a str,
    s: &'a [u8],
    pos: usize,
    end: usize,
}

impl<'a> Tokenizer<'a> {
    /// Positions are `u32`: a source over 4 GiB is refused here.
    pub fn new(src: &'a str) -> Self {
        Self::in_span(src, Span::new(0, src.len()))
    }

    /// Tokenize only `src[span]`, as if the input ended at its end.
    pub fn in_span(src: &'a str, span: Span) -> Self {
        assert!(src.len() <= u32::MAX as usize, "source over 4 GiB");
        Tokenizer {
            src,
            s: src.as_bytes(),
            pos: span.start as usize,
            end: span.end as usize,
        }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    /// Go back to a position an earlier token started or ended at.
    pub fn reset(&mut self, pos: usize) {
        self.pos = pos;
    }

    pub fn end(&self) -> usize {
        self.end
    }

    fn at(&self, i: usize) -> u16 {
        if i < self.end { self.s[i] as u16 } else { EOF }
    }

    fn valid_escape(&self, i: usize) -> bool {
        self.at(i) == b'\\' as u16 && !is_newline(self.at(i + 1))
    }

    fn starts_ident(&self, i: usize) -> bool {
        let c = self.at(i);
        if c == b'-' as u16 {
            let n = self.at(i + 1);
            is_ident_start(n) || n == b'-' as u16 || self.valid_escape(i + 1)
        } else if c == b'\\' as u16 {
            self.valid_escape(i)
        } else {
            is_ident_start(c)
        }
    }

    fn starts_number(&self, i: usize) -> bool {
        let c = self.at(i);
        if c == b'+' as u16 || c == b'-' as u16 {
            let n = self.at(i + 1);
            is_digit(n) || (n == b'.' as u16 && is_digit(self.at(i + 2)))
        } else if c == b'.' as u16 {
            is_digit(self.at(i + 1))
        } else {
            is_digit(c)
        }
    }

    fn skip_comments(&mut self) {
        while self.at(self.pos) == b'/' as u16 && self.at(self.pos + 1) == b'*' as u16 {
            let from = (self.pos + 2).min(self.end);
            self.pos = match self.s[from..self.end].windows(2).position(|w| w == b"*/") {
                Some(k) => from + k + 2,
                None => self.end,
            };
        }
    }

    /// Past the escape whose backslash is at `pos - 1`.
    fn skip_escape(&mut self) {
        let c = self.at(self.pos);
        if is_hex(c) {
            let mut k = 0;
            while k < 6 && is_hex(self.at(self.pos)) {
                self.pos += 1;
                k += 1;
            }
            self.skip_one_whitespace();
        } else if c != EOF {
            self.pos += 1;
        }
    }

    fn skip_one_whitespace(&mut self) {
        let w = self.at(self.pos);
        if is_whitespace(w) {
            self.pos += 1;
            if w == 0x0D && self.at(self.pos) == 0x0A {
                self.pos += 1;
            }
        }
    }

    /// Consume an ident sequence; whether it had an escape or a NUL.
    fn ident_sequence(&mut self) -> bool {
        let mut escaped = false;
        loop {
            let c = self.at(self.pos);
            if is_ident_char(c) {
                escaped |= c == 0;
                self.pos += 1;
            } else if self.valid_escape(self.pos) {
                escaped = true;
                self.pos += 1;
                self.skip_escape();
            } else {
                return escaped;
            }
        }
    }

    pub fn next_token(&mut self) -> Token {
        self.skip_comments();
        let start = self.pos;
        let t = self.token(start);
        step!(self.pos - start + 1);
        t
    }

    fn token(&mut self, start: usize) -> Token {
        let c = self.at(start);
        let simple = |k: Kind, len: usize| (k, len);
        let (kind, len) = match c {
            EOF => return self.make(Kind::Eof, start, start, 0),
            0x09 | 0x0A | 0x0C | 0x0D | 0x20 => {
                let mut i = start;
                while is_whitespace(self.at(i)) {
                    i += 1;
                }
                (Kind::Whitespace, i - start)
            }
            0x22 | 0x27 => return self.string(start, c),
            0x23 => {
                if is_ident_char(self.at(start + 1)) || self.valid_escape(start + 1) {
                    let id = self.starts_ident(start + 1);
                    self.pos = start + 1;
                    let escaped = self.ident_sequence();
                    let mut t = self.make(Kind::Hash { id }, start, start + 1, 0);
                    t.flags |= if escaped { ESCAPED } else { 0 };
                    return t;
                }
                simple(Kind::Delim(b'#'), 1)
            }
            0x28 => simple(Kind::OpenParen, 1),
            0x29 => simple(Kind::CloseParen, 1),
            0x2C => simple(Kind::Comma, 1),
            0x3A => simple(Kind::Colon, 1),
            0x3B => simple(Kind::Semicolon, 1),
            0x5B => simple(Kind::OpenSquare, 1),
            0x5D => simple(Kind::CloseSquare, 1),
            0x7B => simple(Kind::OpenCurly, 1),
            0x7D => simple(Kind::CloseCurly, 1),
            0x2B | 0x2E if self.starts_number(start) => return self.numeric(start),
            0x2D => {
                if self.starts_number(start) {
                    return self.numeric(start);
                }
                if self.at(start + 1) == b'-' as u16 && self.at(start + 2) == b'>' as u16 {
                    simple(Kind::Cdc, 3)
                } else if self.starts_ident(start) {
                    return self.ident_like(start);
                } else {
                    simple(Kind::Delim(b'-'), 1)
                }
            }
            0x3C if self.s.get(start + 1..start + 4) == Some(b"!--") && start + 4 <= self.end => {
                simple(Kind::Cdo, 4)
            }
            0x40 => {
                if self.starts_ident(start + 1) {
                    self.pos = start + 1;
                    let escaped = self.ident_sequence();
                    let mut t = self.make(Kind::AtKeyword, start, start + 1, 0);
                    t.flags |= if escaped { ESCAPED } else { 0 };
                    return t;
                }
                simple(Kind::Delim(b'@'), 1)
            }
            0x5C if self.valid_escape(start) => return self.ident_like(start),
            0x30..=0x39 => return self.numeric(start),
            0x55 | 0x75
                if self.at(start + 1) == b'+' as u16
                    && (is_hex(self.at(start + 2)) || self.at(start + 2) == b'?' as u16) =>
            {
                return self.unicode_range(start);
            }
            0x7E | 0x7C | 0x5E | 0x24 | 0x2A if self.at(start + 1) == b'=' as u16 => {
                let k = match c as u8 {
                    b'~' => Kind::IncludeMatch,
                    b'|' => Kind::DashMatch,
                    b'^' => Kind::PrefixMatch,
                    b'$' => Kind::SuffixMatch,
                    _ => Kind::SubstringMatch,
                };
                simple(k, 2)
            }
            0x7C if self.at(start + 1) == b'|' as u16 => simple(Kind::Column, 2),
            _ if is_ident_start(c) => return self.ident_like(start),
            _ => simple(Kind::Delim(c as u8), 1),
        };
        self.pos = start + len;
        self.make(kind, start, start, 0)
    }

    /// A token from `start` to the current position whose value starts
    /// at `value_start` and runs to the current position.
    fn make(&self, kind: Kind, start: usize, value_start: usize, flags: u8) -> Token {
        Token {
            kind,
            span: Span::new(start, self.pos),
            value: Span::new(value_start, self.pos),
            unit_start: self.pos as u32,
            flags,
        }
    }
}
