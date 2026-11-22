//! The `An+B` microsyntax of CSS Syntax Level 3 (6.2), over the tokens
//! of a span: `odd`, `even`, an integer, or an `n` term written as a
//! dimension, an identifier or `+` and an identifier, with an optional
//! `+ B` or `- B` after it.

use crate::Span;
use crate::tokenizer::{INTEGER, Kind, Token, Tokenizer};

/// What an `n` term's name is after its `n`.
enum NTerm {
    N,
    /// `n-`: the B comes as a separate signless integer
    NDash,
    /// `n-<digits>`: B is minus the digits
    NDashDigits(i32),
}

fn n_term(mut chars: impl Iterator<Item = char>) -> Option<NTerm> {
    if !matches!(chars.next(), Some('n' | 'N')) {
        return None;
    }
    match chars.next() {
        None => return Some(NTerm::N),
        Some('-') => {}
        Some(_) => return None,
    }
    let mut digits: Option<i64> = None;
    for c in chars {
        let d = c.to_digit(10)? as i64;
        digits = Some((digits.unwrap_or(0) * 10 + d).min(i32::MAX as i64 + 1));
    }
    Some(match digits {
        None => NTerm::NDash,
        Some(v) => NTerm::NDashDigits(clamp(-v)),
    })
}

fn clamp(v: i64) -> i32 {
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn integer(t: &Token, src: &str) -> Option<i32> {
    (t.kind == Kind::Number && t.is(INTEGER)).then(|| clamp(t.number(src) as i64))
}

fn signed(t: &Token, src: &str) -> bool {
    matches!(t.value.of(src).as_bytes().first(), Some(b'+' | b'-'))
}

struct Toks<'a> {
    src: &'a str,
    tok: Tokenizer<'a>,
}

impl Toks<'_> {
    fn next_non_ws(&mut self) -> Token {
        loop {
            let t = self.tok.next_token();
            if t.kind != Kind::Whitespace {
                return t;
            }
        }
    }

    /// B after an `n` term of the given form, then the end of the input.
    fn finish(&mut self, a: i32, term: NTerm) -> Option<(i32, i32)> {
        let src = self.src;
        let b = match term {
            NTerm::NDashDigits(b) => b,
            NTerm::NDash => {
                let t = self.next_non_ws();
                if signed(&t, src) {
                    return None;
                }
                integer(&t, src)?.checked_neg()?
            }
            NTerm::N => {
                let t = self.next_non_ws();
                match t.kind {
                    Kind::Eof => return Some((a, 0)),
                    Kind::Number if signed(&t, src) => integer(&t, src)?,
                    Kind::Delim(s @ (b'+' | b'-')) => {
                        let n = self.next_non_ws();
                        if signed(&n, src) {
                            return None;
                        }
                        let v = integer(&n, src)?;
                        if s == b'-' { clamp(-(v as i64)) } else { v }
                    }
                    _ => return None,
                }
            }
        };
        (self.next_non_ws().kind == Kind::Eof).then_some((a, b))
    }
}

/// `(A, B)`, or `None` when the span is not `An+B`.
pub fn parse_anb(src: &str, span: Span) -> Option<(i32, i32)> {
    let mut t = Toks {
        src,
        tok: Tokenizer::in_span(src, span),
    };
    let first = t.next_non_ws();
    match first.kind {
        Kind::Ident if first.value_is(src, "odd") || first.value_is(src, "even") => {
            let b = if first.value_is(src, "odd") { 1 } else { 0 };
            (t.next_non_ws().kind == Kind::Eof).then_some((2, b))
        }
        Kind::Number => {
            let v = integer(&first, src)?;
            (t.next_non_ws().kind == Kind::Eof).then_some((0, v))
        }
        Kind::Dimension if first.is(INTEGER) => {
            let a = clamp(first.number(src) as i64);
            let term = n_term(first.unit_chars(src))?;
            t.finish(a, term)
        }
        Kind::Ident => {
            let mut chars = first.chars(src).peekable();
            let a = if chars.peek() == Some(&'-') {
                chars.next();
                -1
            } else {
                1
            };
            let term = n_term(chars)?;
            t.finish(a, term)
        }
        Kind::Delim(b'+') => {
            let n = t.tok.next_token();
            if n.kind != Kind::Ident {
                return None;
            }
            let term = n_term(n.chars(src))?;
            t.finish(1, term)
        }
        _ => None,
    }
}
