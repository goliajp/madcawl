//! Line break opportunities (UAX #14), the default rules without tailoring.

use crate::{GeneralCategory as Gc, LineBreak as L, Source, props};

/// A line break opportunity before the character at `pos`; `mandatory` for a hard break (after BK, CR, LF,
/// NL, and at the end of the text).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Break {
    pub pos: usize,
    pub mandatory: bool,
}

// a character with the combining marks and joiners rule LB9 folds into it
#[derive(Clone, Copy)]
struct Unit {
    class: L,
    category: Gc,
    wide: bool,
    pict: bool,
    dotted_circle: bool,
    ends_zwj: bool,
    start: usize,
    end: usize,
}

/// The line break opportunities of a text, in order; the last is the end of the text. An empty text has none.
pub struct LineBreaks<'a, S: Source + ?Sized> {
    src: &'a S,
    // units after the boundary being decided, read ahead; `scan` is where the next one starts
    ahead: [Option<Unit>; 3],
    scan: usize,
    a: Option<Unit>,
    a2: Option<Unit>,
    // rules that see through spaces: ZW SP*, OP SP*, initial quote SP*, (CL | CP) SP*, B2 SP*
    zw_sp: bool,
    op_sp: bool,
    pi_sp: bool,
    close_sp: bool,
    b2_sp: bool,
    // LB25: 1 after NU (SY | IS)*, 2 after NU (SY | IS)* (CL | CP)
    num: u8,
    ri_odd: bool,
}

// LB1 with the default resolution of AI, SG, XX, SA and CJ
fn resolve(cp: u32) -> (L, &'static crate::Props) {
    let p = props(cp);
    let c = match p.line {
        L::AI | L::SG | L::XX => L::AL,
        L::SA if matches!(p.category, Gc::Mn | Gc::Mc) => L::CM,
        L::SA => L::AL,
        L::CJ => L::NS,
        c => c,
    };
    (c, p)
}

fn unit<S: Source + ?Sized>(src: &S, start: usize) -> Option<Unit> {
    if start >= src.len() {
        return None;
    }
    let (cp, mut end) = src.next(start);
    let (class, p) = resolve(cp);
    let mut u = Unit {
        class,
        category: p.category,
        wide: p.east_asian_wide(),
        pict: p.extended_pictographic(),
        dotted_circle: cp == 0x25CC,
        ends_zwj: class == L::ZWJ,
        start,
        end,
    };
    if matches!(class, L::CM | L::ZWJ) {
        // LB10: as if U+0041
        u.class = L::AL;
        u.category = Gc::Lu;
        u.wide = false;
        u.pict = false;
    }
    if !matches!(class, L::BK | L::CR | L::LF | L::NL | L::SP | L::ZW) {
        while end < src.len() {
            let (cp, e) = src.next(end);
            let c = resolve(cp).0;
            if !matches!(c, L::CM | L::ZWJ) {
                break;
            }
            u.ends_zwj = c == L::ZWJ;
            end = e;
        }
    }
    u.end = end;
    Some(u)
}

mod special;

#[derive(PartialEq)]
enum Verdict {
    No,
    May,
    Must,
}

impl<'a, S: Source + ?Sized> LineBreaks<'a, S> {
    pub fn new(src: &'a S) -> Self {
        LineBreaks {
            src,
            ahead: [None; 3],
            scan: 0,
            a: None,
            a2: None,
            zw_sp: false,
            op_sp: false,
            pi_sp: false,
            close_sp: false,
            b2_sp: false,
            num: 0,
            ri_odd: false,
        }
    }

    fn peek(&mut self, k: usize) -> Option<Unit> {
        for i in 0..=k {
            if self.ahead[i].is_none() {
                let u = unit(self.src, self.scan)?;
                self.ahead[i] = Some(u);
                self.scan = u.end;
            }
        }
        self.ahead[k]
    }

    fn pop(&mut self) -> Option<Unit> {
        let u = self.peek(0);
        self.ahead = [self.ahead[1], self.ahead[2], None];
        u
    }

    fn decide(&mut self, a: &Unit, b: &Unit) -> Verdict {
        use L::*;
        let (x, y) = (a.class, b.class);
        if x == BK {
            return Verdict::Must;
        }
        if x == CR && y == LF {
            return Verdict::No;
        }
        if matches!(x, CR | LF | NL) {
            return Verdict::Must;
        }
        if matches!(y, BK | CR | LF | NL | SP | ZW) {
            return Verdict::No;
        }
        if self.zw_sp {
            return Verdict::May;
        }
        if a.ends_zwj || x == WJ || y == WJ || x == GL || (y == GL && !matches!(x, SP | HY | HH)) {
            return Verdict::No;
        }
        if matches!(y, CL | CP | EX | SY) || self.op_sp || self.pi_sp {
            return Verdict::No;
        }
        if y == QU && b.category == Gc::Pf {
            let n = self.peek(1).map(|n| n.class);
            if n.is_none_or(|n| {
                matches!(
                    n,
                    SP | GL | WJ | CL | QU | CP | EX | IS | SY | BK | CR | LF | NL | ZW
                )
            }) {
                return Verdict::No;
            }
        }
        if x == SP && y == IS && self.peek(1).is_some_and(|n| n.class == NU) {
            return Verdict::May;
        }
        if y == IS || (self.close_sp && y == NS) || (self.b2_sp && y == B2) {
            return Verdict::No;
        }
        if x == SP {
            return Verdict::May;
        }
        self.special(a, b)
    }

    fn take(&mut self, b: Unit) {
        use L::*;
        let y = b.class;
        let sp = y == SP;
        self.zw_sp = y == ZW || (sp && self.zw_sp);
        self.op_sp = y == OP || (sp && self.op_sp);
        let opens = y == QU
            && b.category == Gc::Pi
            && self
                .a
                .is_none_or(|p| matches!(p.class, BK | CR | LF | NL | OP | QU | GL | SP | ZW));
        self.pi_sp = opens || (sp && self.pi_sp);
        self.close_sp = matches!(y, CL | CP) || (sp && self.close_sp);
        self.b2_sp = y == B2 || (sp && self.b2_sp);
        self.num = match y {
            NU => 1,
            SY | IS if self.num == 1 => 1,
            CL | CP if self.num == 1 => 2,
            _ => 0,
        };
        self.ri_odd = y == RI && !self.ri_odd;
        self.a2 = self.a;
        self.a = Some(b);
    }
}

impl<S: Source + ?Sized> Iterator for LineBreaks<'_, S> {
    type Item = Break;

    fn next(&mut self) -> Option<Break> {
        loop {
            let Some(b) = self.peek(0) else {
                // LB3
                return self.a.take().map(|_| Break {
                    pos: self.src.len(),
                    mandatory: true,
                });
            };
            let v = match self.a {
                Some(a) => self.decide(&a, &b),
                None => Verdict::No,
            };
            self.pop();
            self.take(b);
            if v != Verdict::No {
                return Some(Break {
                    pos: b.start,
                    mandatory: v == Verdict::Must,
                });
            }
        }
    }
}
