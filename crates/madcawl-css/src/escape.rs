//! The byte classes the tokenizer works with, and the decoding of a
//! token's value: escapes, and U+0000, which the input preprocessing
//! turns into U+FFFD. The tokenizer keeps values as spans of the
//! source; only a value with a backslash or a NUL in it needs decoding.

pub(crate) const EOF: u16 = 256;

pub(crate) fn is_newline(c: u16) -> bool {
    matches!(c, 0x0A | 0x0C | 0x0D)
}

pub(crate) fn is_whitespace(c: u16) -> bool {
    matches!(c, 0x09 | 0x0A | 0x0C | 0x0D | 0x20)
}

pub(crate) fn is_digit(c: u16) -> bool {
    (b'0' as u16..=b'9' as u16).contains(&c)
}

pub(crate) fn is_hex(c: u16) -> bool {
    c < 0x80 && (c as u8).is_ascii_hexdigit()
}

/// Every non-ASCII code point starts an identifier, and so does NUL,
/// which stands for U+FFFD.
pub(crate) fn is_ident_start(c: u16) -> bool {
    c != EOF && (c >= 0x80 || c == 0 || c == b'_' as u16 || (c as u8).is_ascii_alphabetic())
}

pub(crate) fn is_ident_char(c: u16) -> bool {
    is_ident_start(c) || is_digit(c) || c == b'-' as u16
}

pub(crate) fn is_non_printable(c: u16) -> bool {
    matches!(c, 0x01..=0x08 | 0x0B | 0x0E..=0x1F | 0x7F)
}

pub(crate) fn hex_value(b: u8) -> u32 {
    (b as char).to_digit(16).unwrap_or(0)
}

/// The code point a hex escape names, U+FFFD for one no character has.
pub(crate) fn escaped_char(v: u32) -> char {
    match v {
        0 | 0xD800..=0xDFFF => '\u{FFFD}',
        _ => char::from_u32(v).unwrap_or('\u{FFFD}'),
    }
}

/// The character at `i` of valid UTF-8 and its length.
pub(crate) fn char_at(s: &[u8], i: usize) -> (char, usize) {
    let b = s[i] as u32;
    let (len, init) = match b {
        0..=0x7F => return (b as u8 as char, 1),
        0xC0..=0xDF => (2, b & 0x1F),
        0xE0..=0xEF => (3, b & 0x0F),
        _ => (4, b & 0x07),
    };
    let v = s[i + 1..i + len]
        .iter()
        .fold(init, |v, &c| (v << 6) | (c as u32 & 0x3F));
    (char::from_u32(v).unwrap_or('\u{FFFD}'), len)
}

/// The decoded characters of a token value `s[i..end]`. In a string an
/// escaped newline is dropped and a backslash at the end of the input is
/// nothing; anywhere else that backslash is U+FFFD.
pub struct Unescape<'a> {
    s: &'a [u8],
    i: usize,
    end: usize,
    string: bool,
}

impl<'a> Unescape<'a> {
    pub(crate) fn new(s: &'a [u8], i: usize, end: usize, string: bool) -> Self {
        Unescape { s, i, end, string }
    }

    fn at(&self, i: usize) -> u16 {
        if i < self.end { self.s[i] as u16 } else { EOF }
    }
}

impl Iterator for Unescape<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        loop {
            let c = self.at(self.i);
            if c == EOF {
                return None;
            }
            if c == 0 {
                self.i += 1;
                return Some('\u{FFFD}');
            }
            if c != b'\\' as u16 {
                let (ch, len) = char_at(self.s, self.i);
                self.i += len;
                return Some(ch);
            }
            self.i += 1;
            let n = self.at(self.i);
            if n == EOF {
                return if self.string { None } else { Some('\u{FFFD}') };
            }
            if is_newline(n) {
                self.i += 1;
                if n == 0x0D && self.at(self.i) == 0x0A {
                    self.i += 1;
                }
                continue;
            }
            if is_hex(n) {
                let mut v = 0u32;
                let mut k = 0;
                while k < 6 && is_hex(self.at(self.i)) {
                    v = v * 16 + hex_value(self.s[self.i]);
                    self.i += 1;
                    k += 1;
                }
                let w = self.at(self.i);
                if is_whitespace(w) {
                    self.i += 1;
                    if w == 0x0D && self.at(self.i) == 0x0A {
                        self.i += 1;
                    }
                }
                return Some(escaped_char(v));
            }
            if n == 0 {
                self.i += 1;
                return Some('\u{FFFD}');
            }
            let (ch, len) = char_at(self.s, self.i);
            self.i += len;
            return Some(ch);
        }
    }
}
