//! The tokens: a kind, spans of the source and flags.

use crate::Span;
use crate::escape::Unescape;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ident,
    Function,
    AtKeyword,
    /// `id` when the name would start an identifier
    Hash {
        id: bool,
    },
    String,
    BadString,
    Url,
    BadUrl,
    /// One ASCII code point the grammar has no other token for
    Delim(u8),
    Number,
    Percentage,
    Dimension,
    Whitespace,
    Cdo,
    Cdc,
    Colon,
    Semicolon,
    Comma,
    OpenSquare,
    CloseSquare,
    OpenParen,
    CloseParen,
    OpenCurly,
    CloseCurly,
    IncludeMatch,
    DashMatch,
    PrefixMatch,
    SuffixMatch,
    SubstringMatch,
    Column,
    Eof,
}

/// The value has a backslash or a NUL: read it with `Token::chars`
pub const ESCAPED: u8 = 1;
/// A number, percentage or dimension written without `.` or exponent
pub const INTEGER: u8 = 2;
/// A string or url the end of the input cut short
pub const AT_EOF: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    /// The whole token
    pub span: Span,
    /// The name of an identifier, function, at-keyword or hash; the
    /// contents of a string or url; the number of a numeric token as
    /// written
    pub value: Span,
    /// Where a dimension's unit starts; it ends where the token does
    pub unit_start: u32,
    pub flags: u8,
}

impl Token {
    pub fn is(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    pub fn unit(&self) -> Span {
        Span::new(self.unit_start as usize, self.span.end as usize)
    }

    /// The numeric value of a number, percentage or dimension; a value
    /// too large to represent is clamped to the largest finite one.
    pub fn number(&self, src: &str) -> f64 {
        let repr = self.value.of(src);
        let v: f64 = repr.parse().unwrap_or_else(|_| {
            // a mantissa `repr` cannot parse is a long run of digits
            // ending in `.`, which the tokenizer never produces
            unreachable!("number token {repr:?}")
        });
        v.clamp(f64::MIN, f64::MAX)
    }

    /// The decoded characters of the value.
    pub fn chars<'a>(&self, src: &'a str) -> Unescape<'a> {
        self.span_chars(src, self.value)
    }

    /// The decoded characters of the unit.
    pub fn unit_chars<'a>(&self, src: &'a str) -> Unescape<'a> {
        self.span_chars(src, self.unit())
    }

    fn span_chars<'a>(&self, src: &'a str, s: Span) -> Unescape<'a> {
        let string = matches!(self.kind, Kind::String | Kind::BadString);
        Unescape::new(src.as_bytes(), s.start as usize, s.end as usize, string)
    }

    /// The value as it decodes, appended to `out`.
    pub fn write_value(&self, src: &str, out: &mut String) {
        if self.is(ESCAPED) {
            out.extend(self.chars(src));
        } else {
            out.push_str(self.value.of(src));
        }
    }

    /// Whether the value decodes to `name`, ASCII case-insensitively.
    pub fn value_is(&self, src: &str, name: &str) -> bool {
        if self.is(ESCAPED) {
            let mut a = self.chars(src);
            let mut b = name.chars();
            loop {
                match (a.next(), b.next()) {
                    (None, None) => return true,
                    (Some(x), Some(y)) if x.eq_ignore_ascii_case(&y) => {}
                    _ => return false,
                }
            }
        }
        self.value.of(src).eq_ignore_ascii_case(name)
    }

    /// Whether the dimension's unit decodes to `name`, ASCII
    /// case-insensitively.
    pub fn unit_is(&self, src: &str, name: &str) -> bool {
        let mut a = self.unit_chars(src);
        let mut b = name.chars();
        loop {
            match (a.next(), b.next()) {
                (None, None) => return true,
                (Some(x), Some(y)) if x.eq_ignore_ascii_case(&y) => {}
                _ => return false,
            }
        }
    }
}
