//! Default word boundaries (UAX #29 section 4).

use crate::{Props, Source, WordBreak as W, props};

#[derive(Clone, Copy)]
struct Unit {
    class: W,
    pict: bool,
    // the unit is a single character, not followed by ignored ones (rules WB3, WB3d look at raw neighbours)
    bare: bool,
    ends_zwj: bool,
    start: usize,
    end: usize,
}

/// The default word boundaries of a text: every offset after the start where a boundary falls, then the end
/// of the text. An empty text has none.
pub struct WordBreaks<'a, S: Source + ?Sized> {
    src: &'a S,
    // the last two units before the boundary being decided, and the unit after it
    a: Option<Unit>,
    a2: Option<W>,
    b: Option<Unit>,
    ri_odd: bool,
    done: bool,
}

pub fn word_breaks<S: Source + ?Sized>(src: &S) -> WordBreaks<'_, S> {
    let b = unit(src, 0);
    WordBreaks {
        src,
        a: None,
        a2: None,
        b,
        ri_odd: false,
        done: false,
    }
}

fn ignored(w: W) -> bool {
    matches!(w, W::Extend | W::Format | W::ZWJ)
}

// a character and the Extend / Format / ZWJ characters rule WB4 folds into it
fn unit<S: Source + ?Sized>(src: &S, start: usize) -> Option<Unit> {
    if start >= src.len() {
        return None;
    }
    let (cp, mut end) = src.next(start);
    let p: &Props = props(cp);
    let mut u = Unit {
        class: p.word,
        pict: p.extended_pictographic(),
        bare: true,
        ends_zwj: p.word == W::ZWJ,
        start,
        end,
    };
    if matches!(p.word, W::CR | W::LF | W::Newline) {
        return Some(u);
    }
    while end < src.len() {
        let (cp, e) = src.next(end);
        let w = props(cp).word;
        if !ignored(w) {
            break;
        }
        u.bare = false;
        u.ends_zwj = w == W::ZWJ;
        end = e;
    }
    u.end = end;
    Some(u)
}

fn ah(w: W) -> bool {
    matches!(w, W::ALetter | W::HebrewLetter)
}

fn mid_num_let_q(w: W) -> bool {
    matches!(w, W::MidNumLet | W::SingleQuote)
}

impl<S: Source + ?Sized> WordBreaks<'_, S> {
    fn joins(&self, a: &Unit, b: &Unit) -> bool {
        let next = || unit(self.src, b.end).map(|u| u.class);
        let (x, y) = (a.class, b.class);
        match (x, y) {
            (W::CR, W::LF) => return true,
            (W::Newline | W::CR | W::LF, _) | (_, W::Newline | W::CR | W::LF) => return false,
            _ => {}
        }
        if a.ends_zwj && b.pict || a.bare && x == W::WSegSpace && y == W::WSegSpace {
            return true;
        }
        let a2 = self.a2;
        (ah(x) && ah(y))
            || (ah(x) && (y == W::MidLetter || mid_num_let_q(y)) && next().is_some_and(ah))
            || (a2.is_some_and(ah) && (x == W::MidLetter || mid_num_let_q(x)) && ah(y))
            || (x == W::HebrewLetter && y == W::SingleQuote)
            || (x == W::HebrewLetter && y == W::DoubleQuote && next() == Some(W::HebrewLetter))
            || (a2 == Some(W::HebrewLetter) && x == W::DoubleQuote && y == W::HebrewLetter)
            || (x == W::Numeric && y == W::Numeric)
            || (ah(x) && y == W::Numeric)
            || (x == W::Numeric && ah(y))
            || (a2 == Some(W::Numeric) && (x == W::MidNum || mid_num_let_q(x)) && y == W::Numeric)
            || (x == W::Numeric
                && (y == W::MidNum || mid_num_let_q(y))
                && next() == Some(W::Numeric))
            || (x == W::Katakana && y == W::Katakana)
            || (matches!(
                x,
                W::ALetter | W::HebrewLetter | W::Numeric | W::Katakana | W::ExtendNumLet
            ) && y == W::ExtendNumLet)
            || (x == W::ExtendNumLet
                && matches!(y, W::ALetter | W::HebrewLetter | W::Numeric | W::Katakana))
            || (x == W::RegionalIndicator && y == W::RegionalIndicator && self.ri_odd)
    }
}

impl<S: Source + ?Sized> Iterator for WordBreaks<'_, S> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        loop {
            let Some(b) = self.b else {
                if self.done || self.a.is_none() {
                    return None;
                }
                self.done = true;
                return Some(self.src.len());
            };
            let brk = match &self.a {
                Some(a) => !self.joins(a, &b),
                None => false,
            };
            self.ri_odd = b.class == W::RegionalIndicator && !self.ri_odd;
            self.a2 = self.a.map(|u| u.class);
            self.a = Some(b);
            self.b = unit(self.src, b.end);
            if brk {
                return Some(b.start);
            }
        }
    }
}
