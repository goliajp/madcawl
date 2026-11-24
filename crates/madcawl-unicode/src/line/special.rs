//! Rules LB19 to LB31.

use super::{LineBreaks, Unit, Verdict};
use crate::{GeneralCategory as Gc, LineBreak as L, Source};

impl<S: Source + ?Sized> LineBreaks<'_, S> {
    // LB19 to LB22
    pub(super) fn special(&mut self, a: &Unit, b: &Unit) -> Verdict {
        use L::*;
        let (x, y) = (a.class, b.class);
        let a2 = self.a2;
        if (y == QU && b.category != Gc::Pi) || (x == QU && a.category != Gc::Pf) {
            return Verdict::No;
        }
        if y == QU && (!a.wide || self.peek(1).is_none_or(|n| !n.wide)) {
            return Verdict::No;
        }
        if x == QU && (!b.wide || a2.is_none_or(|p| !p.wide)) {
            return Verdict::No;
        }
        if x == CB || y == CB {
            return Verdict::May;
        }
        let hyphen = matches!(x, HY | HH);
        if hyphen
            && matches!(y, AL | HL)
            && a2.is_none_or(|p| matches!(p.class, BK | CR | LF | NL | SP | ZW | CB | GL))
        {
            return Verdict::No;
        }
        if matches!(y, BA | HH | HY | NS) || x == BB {
            return Verdict::No;
        }
        if (hyphen && a2.is_some_and(|p| p.class == HL) && y != HL)
            || (x == SY && y == HL)
            || y == IN
        {
            return Verdict::No;
        }
        self.numeric_and_alphabetic(a, b)
    }

    // LB23 to LB31
    fn numeric_and_alphabetic(&mut self, a: &Unit, b: &Unit) -> Verdict {
        use L::*;
        let (x, y) = (a.class, b.class);
        let alpha = |c| matches!(c, AL | HL);
        let ideo = |c| matches!(c, ID | EB | EM);
        let hangul = |c| matches!(c, JL | JV | JT | H2 | H3);
        if (alpha(x) && y == NU)
            || (x == NU && alpha(y))
            || (x == PR && ideo(y))
            || (ideo(x) && y == PO)
        {
            return Verdict::No;
        }
        let affix = |c| matches!(c, PR | PO);
        if (affix(x) && alpha(y)) || (alpha(x) && affix(y)) || self.number(x, y) {
            return Verdict::No;
        }
        let korean = (x == JL && matches!(y, JL | JV | H2 | H3))
            || (matches!(x, JV | H2) && matches!(y, JV | JT))
            || (matches!(x, JT | H3) && y == JT)
            || (hangul(x) && y == PO)
            || (x == PR && hangul(y));
        if korean || (alpha(x) && alpha(y)) || self.aksara(a, b) || (x == IS && alpha(y)) {
            return Verdict::No;
        }
        if (matches!(x, AL | HL | NU) && y == OP && !b.wide)
            || (x == CP && !a.wide && matches!(y, AL | HL | NU))
        {
            return Verdict::No;
        }
        if (x == RI && y == RI && self.ri_odd)
            || (y == EM && (x == EB || (a.pict && a.category == Gc::Cn)))
        {
            return Verdict::No;
        }
        Verdict::May
    }

    // LB25
    fn number(&mut self, x: L, y: L) -> bool {
        use L::*;
        if (self.num == 2 && matches!(y, PO | PR)) || (self.num == 1 && matches!(y, PO | PR | NU)) {
            return true;
        }
        if matches!(x, PO | PR) && y == OP {
            return match self.peek(1).map(|n| n.class) {
                Some(NU) => true,
                Some(IS) => self.peek(2).is_some_and(|n| n.class == NU),
                _ => false,
            };
        }
        matches!(x, PO | PR | HY | IS) && y == NU
    }

    // LB28a
    fn aksara(&mut self, a: &Unit, b: &Unit) -> bool {
        use L::*;
        let base = |u: &Unit| matches!(u.class, AK | AS) || u.dotted_circle;
        if (a.class == AP && base(b)) || (base(a) && matches!(b.class, VF | VI)) {
            return true;
        }
        if a.class == VI && self.a2.is_some_and(|p| base(&p)) && (b.class == AK || b.dotted_circle)
        {
            return true;
        }
        base(a) && base(b) && self.peek(1).is_some_and(|n| n.class == VF)
    }
}
