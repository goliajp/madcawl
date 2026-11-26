//! The input stream: bytes as they arrive, read a code point at a time.
//!
//! The bytes are WTF-8, as a decoder or a script makes them: a debug
//! build stops on a byte that is not, a release build reads it as
//! U+FFFD on its own. CR and CRLF read as LF. Each code point the
//! standard calls a problem in the input stream is reported once, the
//! first time it is read, however often a state reads it again.

use crate::error::ParseError;

/// The end of the input, read after the last code point.
pub(crate) const EOF: u32 = u32::MAX;
/// Not enough input to say what comes next; the caller waits for more.
pub(crate) const MORE: u32 = u32::MAX - 1;

const REPLACEMENT: u32 = 0xFFFD;

#[derive(Default)]
pub(crate) struct Input {
    buf: Vec<u8>,
    pos: usize,
    /// Where the last code point read started, for `unread`.
    prev: usize,
    done: bool,
    /// Bytes before this have had their problems reported.
    checked: usize,
    pub(crate) error: Option<ParseError>,
    /// Insertion points, innermost last: where `document.write` puts
    /// its text while a script the parser stopped for runs.
    points: Vec<usize>,
}

impl Input {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        // what is behind the last code point can never be read again
        if self.prev > 4096 && self.prev * 2 > self.buf.len() {
            let cut = self.prev;
            self.buf.drain(..cut);
            self.pos -= cut;
            self.prev = 0;
            self.checked = self.checked.saturating_sub(cut);
            for p in &mut self.points {
                *p -= cut;
            }
        }
        self.buf.extend_from_slice(bytes);
    }

    /// A new insertion point just before the next code point.
    pub(crate) fn push_point(&mut self) {
        self.points.push(self.pos);
    }

    pub(crate) fn pop_point(&mut self) {
        self.points.pop();
    }

    pub(crate) fn has_point(&self) -> bool {
        !self.points.is_empty()
    }

    /// `bytes` at the innermost insertion point, which moves past them;
    /// the outer points after it move with the text.
    pub(crate) fn insert(&mut self, bytes: &[u8]) {
        let at = *self.points.last().expect("an insertion point");
        self.buf.splice(at..at, bytes.iter().copied());
        for p in &mut self.points {
            if *p >= at {
                *p += bytes.len();
            }
        }
    }

    /// Run `f` with the input ending at the innermost insertion point,
    /// as the tokenizer runs for `document.write`: what follows it is
    /// set aside and has not arrived.
    pub(crate) fn up_to_point<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let at = *self.points.last().expect("an insertion point");
        let tail = self.buf.split_off(at);
        let done = std::mem::replace(&mut self.done, false);
        let r = f(self);
        self.done = done;
        let cut = self.buf.len() - at;
        self.buf.extend_from_slice(&tail);
        debug_assert_eq!(cut, 0, "nothing arrives while the rest is set aside");
        r
    }

    pub(crate) fn finish(&mut self) {
        self.done = true;
    }

    pub(crate) fn bytes(&self, from: usize, to: usize) -> &[u8] {
        &self.buf[from..to]
    }

    /// What has arrived and not been read.
    pub(crate) fn rest(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    pub(crate) fn done(&self) -> bool {
        self.done
    }

    /// Read the next code point: `EOF` at the end, `MORE` when the
    /// rest has not arrived.
    #[inline]
    pub(crate) fn next(&mut self) -> u32 {
        step!(1);
        let Some(&b) = self.buf.get(self.pos) else {
            // reading the end reads nothing: putting it back stays here
            self.prev = self.pos;
            return if self.done { EOF } else { MORE };
        };
        if b < 0x80 {
            if b == b'\r' {
                let n = match self.buf.get(self.pos + 1) {
                    Some(b'\n') => 2,
                    Some(_) => 1,
                    None if self.done => 1,
                    None => return MORE,
                };
                self.advance(n);
                return u32::from(b'\n');
            }
            self.advance(1);
            if self.pos > self.checked {
                self.checked = self.pos;
                if is_control(u32::from(b)) {
                    self.error = Some(ParseError::ControlCharacterInInputStream);
                }
            }
            return u32::from(b);
        }
        let (c, n) = match decode(&self.buf[self.pos..], self.done) {
            Some(d) => d,
            None => return MORE,
        };
        self.advance(n);
        if self.pos > self.checked {
            self.checked = self.pos;
            self.error = problem(c);
        }
        c
    }

    fn advance(&mut self, n: usize) {
        self.prev = self.pos;
        self.pos += n;
    }

    /// Put back the code point last read.
    #[inline]
    pub(crate) fn unread(&mut self) {
        self.pos = self.prev;
    }

    /// Read a run of code points that pass through as they are: up to
    /// the first ASCII byte marked in `stop`, a problem code point, or
    /// the end of what has arrived. Returns the byte range read.
    #[inline]
    pub(crate) fn run(&mut self, stop: &Stops) -> (usize, usize) {
        let start = self.pos;
        let buf = &self.buf;
        let mut i = start;
        loop {
            while let Some(w) = buf.get(i..i + 4)
                && !(stop[w[0] as usize]
                    | stop[w[1] as usize]
                    | stop[w[2] as usize]
                    | stop[w[3] as usize])
            {
                i += 4;
            }
            let Some(&b) = buf.get(i) else { break };
            if !stop[b as usize] {
                i += 1;
                continue;
            }
            if b < 0x80 {
                break;
            }
            let rest = &buf[i..];
            let cont = |k: usize| rest.get(k).is_some_and(|&t| t & 0xC0 == 0x80);
            // sequences that can be neither a problem nor broken: no C1
            // controls, surrogates or noncharacters start this way
            i += match b {
                0xC3..=0xDF if cont(1) => 2,
                0xE1..=0xEC | 0xEE if cont(1) && cont(2) => {
                    let mut k = 3;
                    while let Some(w) = rest.get(k..k + 3)
                        && matches!(w[0], 0xE1..=0xEC | 0xEE)
                        && w[1] & 0xC0 == 0x80
                        && w[2] & 0xC0 == 0x80
                    {
                        k += 3;
                    }
                    k
                }
                _ => match decode(rest, false) {
                    Some((c, n)) if problem(c).is_none() && c != REPLACEMENT_FOR_INVALID => n,
                    _ => break,
                },
            };
        }
        step!(i - start + 1);
        if i > start {
            self.prev = i;
            self.pos = i;
            self.checked = self.checked.max(i);
        }
        (start, i)
    }

    /// Whether what comes next is `pat`, ASCII, compared without case
    /// when `ci`. `None` when too little has arrived to tell. The next
    /// code point counts as read: if it is a problem it goes to
    /// `report` now. The ones after it are reported when they are read.
    pub(crate) fn starts_with(
        &mut self,
        pat: &[u8],
        ci: bool,
        report: &mut impl FnMut(ParseError),
    ) -> Option<bool> {
        step!(pat.len());
        for (at, &want) in (self.pos..).zip(pat) {
            let Some(&b) = self.buf.get(at) else {
                return if self.done { Some(false) } else { None };
            };
            let n = if b < 0x80 {
                1
            } else {
                match decode(&self.buf[at..], self.done) {
                    Some((_, n)) => n,
                    None => return None,
                }
            };
            if at == self.pos && at + n > self.checked {
                let c = if b < 0x80 {
                    u32::from(b)
                } else {
                    decode(&self.buf[at..], true).map_or(0, |d| d.0)
                };
                if let Some(e) = problem(c) {
                    report(e);
                }
                self.checked = at + n;
            }
            let same = if ci {
                b.eq_ignore_ascii_case(&want)
            } else {
                b == want
            };
            if !same {
                return Some(false);
            }
        }
        Some(true)
    }

    /// Skip `n` ASCII bytes that `starts_with` matched.
    pub(crate) fn skip(&mut self, n: usize) {
        self.pos += n;
        self.prev = self.pos;
        self.checked = self.checked.max(self.pos);
    }
}

/// `run` never passes U+FFFD that stands for bytes that were not WTF-8:
/// the decoder only makes one where the input was broken, and a broken
/// byte is read on its own by `next`.
const REPLACEMENT_FOR_INVALID: u32 = u32::MAX - 2;

/// Decode one non-ASCII code point; `None` when a sequence is cut off
/// by the end of what has arrived and more may come.
#[inline]
fn decode(s: &[u8], done: bool) -> Option<(u32, usize)> {
    let b = s[0];
    let (n, init) = match b {
        0xC2..=0xDF => (2, u32::from(b & 0x1F)),
        0xE0..=0xEF => (3, u32::from(b & 0x0F)),
        0xF0..=0xF4 => (4, u32::from(b & 0x07)),
        _ => return broken(),
    };
    if s.len() < n {
        if !done && s[1..].iter().all(|&c| c & 0xC0 == 0x80) {
            return None;
        }
        return broken();
    }
    let mut c = init;
    for &t in &s[1..n] {
        if t & 0xC0 != 0x80 {
            return broken();
        }
        c = (c << 6) | u32::from(t & 0x3F);
    }
    let min = [0, 0, 0x80, 0x800, 0x1_0000][n];
    if c < min || c > 0x10_FFFF {
        return broken();
    }
    Some((c, n))
}

/// A byte that is not WTF-8: the caller fed bytes no decoder made.
#[cold]
fn broken() -> Option<(u32, usize)> {
    if cfg!(debug_assertions) {
        panic!("the tokenizer's input is not WTF-8: decode it first");
    }
    Some((REPLACEMENT_FOR_INVALID, 1))
}

/// Read a code point decoded by `next`: the broken-byte marker becomes
/// U+FFFD for everyone downstream.
#[inline]
pub(crate) fn visible(c: u32) -> u32 {
    if c == REPLACEMENT_FOR_INVALID {
        REPLACEMENT
    } else {
        c
    }
}

fn is_control(c: u32) -> bool {
    matches!(c, 0x01..=0x08 | 0x0B | 0x0E..=0x1F | 0x7F..=0x9F)
}

fn problem(c: u32) -> Option<ParseError> {
    match c {
        0xD800..=0xDFFF => Some(ParseError::SurrogateInInputStream),
        0xFDD0..=0xFDEF => Some(ParseError::NoncharacterInInputStream),
        _ if c & 0xFFFE == 0xFFFE && c <= 0x10_FFFF => Some(ParseError::NoncharacterInInputStream),
        _ if is_control(c) => Some(ParseError::ControlCharacterInInputStream),
        _ => None,
    }
}

/// A stop table for `run`: a byte marked stops the run. Every byte past
/// ASCII is marked, so that a code point that is not ASCII is looked at.
pub(crate) type Stops = [bool; 256];

/// Stop tables for `run`: the ASCII a text state treats specially,
/// plus CR and the controls the input stream reports.
pub(crate) const fn stops(extra: &[u8]) -> Stops {
    let mut t = [false; 256];
    let mut c = 0;
    while c < 256 {
        t[c] = matches!(c, 0x00..=0x08 | 0x0B | 0x0D..=0x1F | 0x7F..);
        c += 1;
    }
    let mut i = 0;
    while i < extra.len() {
        t[extra[i] as usize] = true;
        i += 1;
    }
    t
}

/// A code point as WTF-8.
#[inline]
pub(crate) fn encode_char(c: u32) -> ([u8; 4], usize) {
    let c = visible(c);
    match c {
        0..=0x7F => ([c as u8, 0, 0, 0], 1),
        0x80..=0x7FF => ([0xC0 | (c >> 6) as u8, 0x80 | (c & 0x3F) as u8, 0, 0], 2),
        0x800..=0xFFFF => (
            [
                0xE0 | (c >> 12) as u8,
                0x80 | ((c >> 6) & 0x3F) as u8,
                0x80 | (c & 0x3F) as u8,
                0,
            ],
            3,
        ),
        _ => (
            [
                0xF0 | (c >> 18) as u8,
                0x80 | ((c >> 12) & 0x3F) as u8,
                0x80 | ((c >> 6) & 0x3F) as u8,
                0x80 | (c & 0x3F) as u8,
            ],
            4,
        ),
    }
}

/// Append a code point as WTF-8.
#[inline]
pub(crate) fn push_char(out: &mut Vec<u8>, c: u32) {
    if c < 0x80 {
        out.push(c as u8);
    } else {
        let (b, n) = encode_char(c);
        out.extend_from_slice(&b[..n]);
    }
}
