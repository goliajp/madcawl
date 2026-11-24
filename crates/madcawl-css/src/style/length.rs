//! Lengths and percentages: as declared, a sum of terms in each unit or
//! a `calc()` tree; as computed, 1/64 px fixed point plus a percentage,
//! which only layout can resolve.

use std::sync::Arc;

use crate::tokenizer::Kind;

use super::vals::Item;

/// 1/64 of a CSS pixel per unit, as layout counts.
pub const UNIT: f32 = 64.0;

/// The units a declared length may be in, one coefficient each.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Linear {
    pub px: f32,
    pub em: f32,
    pub rem: f32,
    pub ex: f32,
    pub ch: f32,
    pub vw: f32,
    pub vh: f32,
    pub vmin: f32,
    pub vmax: f32,
    pub pct: f32,
}

impl Linear {
    fn px(v: f32) -> Linear {
        Linear {
            px: v,
            ..Linear::default()
        }
    }

    pub(crate) fn zip(self, o: Linear, f: impl Fn(f32, f32) -> f32) -> Linear {
        Linear {
            px: f(self.px, o.px),
            em: f(self.em, o.em),
            rem: f(self.rem, o.rem),
            ex: f(self.ex, o.ex),
            ch: f(self.ch, o.ch),
            vw: f(self.vw, o.vw),
            vh: f(self.vh, o.vh),
            vmin: f(self.vmin, o.vmin),
            vmax: f(self.vmax, o.vmax),
            pct: f(self.pct, o.pct),
        }
    }

    pub(crate) fn scale(self, k: f32) -> Linear {
        self.zip(Linear::default(), |a, _| a * k)
    }

    fn negative(&self) -> bool {
        [
            self.px, self.em, self.rem, self.ex, self.ch, self.vw, self.vh, self.vmin, self.vmax,
            self.pct,
        ]
        .iter()
        .any(|v| *v < 0.0)
    }
}

/// A `calc()` tree, kept when it holds `min()`, `max()` or `clamp()`,
/// whose terms may only compare once the font size is known.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Leaf(Linear),
    Sum(Box<Expr>, Box<Expr>),
    Scale(Box<Expr>, f32),
    Min(Vec<Expr>),
    Max(Vec<Expr>),
}

/// How a length was written, which decides how it serializes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Form {
    #[default]
    Len,
    Pct,
    Calc,
}

/// A declared `<length-percentage>`.
#[derive(Clone, Debug, PartialEq)]
pub enum SpecLen {
    Lin { v: Linear, form: Form },
    Expr(Arc<Expr>),
}

/// What resolving a length needs: the element's font size, the root's,
/// and the viewport, in px.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub em: f32,
    pub rem: f32,
    pub vw: f32,
    pub vh: f32,
}

/// A computed `<length-percentage>`: `px` in 1/64 px and a percentage;
/// `form` says which of them it is, `Calc` when both are kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lp {
    pub px: i32,
    pct_bits: u32,
    pub form: Form,
}

impl Lp {
    pub const ZERO: Lp = Lp {
        px: 0,
        pct_bits: 0,
        form: Form::Len,
    };

    pub fn px(v: f32) -> Lp {
        Lp {
            px: to_fixed(v),
            ..Lp::ZERO
        }
    }

    pub fn pct(&self) -> f32 {
        f32::from_bits(self.pct_bits)
    }

    /// Whether layout has to resolve it.
    pub fn has_pct(&self) -> bool {
        self.form != Form::Len
    }

    pub fn px_f32(&self) -> f32 {
        self.px as f32 / UNIT
    }
}

/// Truncated toward zero, as WebKit turns a length into layout units; the slack keeps a value that float
/// arithmetic left just under a multiple of 1/64 px on it. The slack is two ulps of the value as well: an
/// `f32` of a few hundred px is already further than 1/1000 of a unit from the value it stands for, so a
/// fixed slack would truncate a whole unit away from large lengths; past about 2^20 units (16384 px) an
/// `f32` no longer tells units apart reliably and no slack helps. Layout resolves percentages through
/// this too, so computed and used lengths round the same way. At a scale that is not a whole number, a
/// device-pixel length (a snapped border) is not a multiple of 1/64 px and lands on the unit below it.
pub fn to_fixed(v: f32) -> i32 {
    let u = f64::from(v) * f64::from(UNIT);
    let slack = 1e-3 + u.abs() * f64::from(f32::EPSILON) * 2.0;
    (u + slack.copysign(u))
        .trunc()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

fn eval(e: &Expr, c: &Ctx) -> Option<(f32, f32)> {
    Some(match e {
        Expr::Leaf(l) => resolve_linear(l, c),
        Expr::Sum(a, b) => {
            let (a, b) = (eval(a, c)?, eval(b, c)?);
            (a.0 + b.0, a.1 + b.1)
        }
        Expr::Scale(a, k) => {
            let a = eval(a, c)?;
            (a.0 * k, a.1 * k)
        }
        Expr::Min(v) | Expr::Max(v) => {
            let min = matches!(e, Expr::Min(_));
            let mut best: Option<f32> = None;
            for x in v {
                let (px, pct) = eval(x, c)?;
                // comparing a percentage needs layout
                if pct != 0.0 {
                    return None;
                }
                best = Some(match best {
                    None => px,
                    Some(b) if min => b.min(px),
                    Some(b) => b.max(px),
                });
            }
            (best?, 0.0)
        }
    })
}

fn resolve_linear(l: &Linear, c: &Ctx) -> (f32, f32) {
    let px = l.px
        + l.em * c.em
        + l.rem * c.rem
        // no font metrics yet: the x-height and the advance of `0` are
        // taken as half an em
        + (l.ex + l.ch) * c.em * 0.5
        + l.vw * c.vw / 100.0
        + l.vh * c.vh / 100.0
        + l.vmin * c.vw.min(c.vh) / 100.0
        + l.vmax * c.vw.max(c.vh) / 100.0;
    (px, l.pct)
}

impl SpecLen {
    /// px and percentage as floats, not rounded to 1/64 px: font sizes
    /// are kept that way.
    pub fn resolve_float(&self, c: &Ctx) -> Option<(f32, f32)> {
        match self {
            SpecLen::Lin { v, .. } => Some(resolve_linear(v, c)),
            SpecLen::Expr(e) => eval(e, c),
        }
    }

    /// The computed value; `None` when a `min()` or `max()` would have
    /// to compare a percentage. `clamp_neg` clamps a `calc()` that came
    /// out negative where negative values are not allowed.
    pub fn resolve(&self, c: &Ctx, clamp_neg: bool) -> Option<Lp> {
        let (form, (mut px, mut pct)) = match self {
            SpecLen::Lin { v, form } => (*form, resolve_linear(v, c)),
            SpecLen::Expr(e) => (Form::Calc, eval(e, c)?),
        };
        let calc = form == Form::Calc;
        if clamp_neg && calc && pct == 0.0 {
            px = px.max(0.0);
        }
        if !px.is_finite() || !pct.is_finite() {
            px = 0.0;
            pct = 0.0;
        }
        let form = match form {
            Form::Calc if pct == 0.0 => Form::Len,
            Form::Calc if px == 0.0 => Form::Pct,
            f => f,
        };
        Some(Lp {
            px: to_fixed(px),
            pct_bits: pct.to_bits(),
            form,
        })
    }
}

/// Whether percentages are taken, and negative values.
#[derive(Clone, Copy)]
pub struct Accept {
    pub pct: bool,
    pub neg: bool,
}

pub(crate) fn unit(src: &str, it: &Item) -> Option<Linear> {
    let v = it.tok.number(src) as f32;
    let mut l = Linear::default();
    let u = |name: &str| it.tok.unit_is(src, name);
    match () {
        _ if u("px") => l.px = v,
        _ if u("em") => l.em = v,
        _ if u("rem") => l.rem = v,
        _ if u("ex") => l.ex = v,
        _ if u("ch") => l.ch = v,
        _ if u("vw") || u("svw") || u("lvw") || u("dvw") => l.vw = v,
        _ if u("vh") || u("svh") || u("lvh") || u("dvh") => l.vh = v,
        _ if u("vmin") => l.vmin = v,
        _ if u("vmax") => l.vmax = v,
        _ if u("pt") => l.px = v * 4.0 / 3.0,
        _ if u("pc") => l.px = v * 16.0,
        _ if u("in") => l.px = v * 96.0,
        _ if u("cm") => l.px = v * 96.0 / 2.54,
        _ if u("mm") => l.px = v * 96.0 / 25.4,
        _ if u("q") => l.px = v * 96.0 / 101.6,
        _ => return None,
    }
    Some(l)
}

/// One item as a `<length-percentage>` (a dimension, a percentage, `0`
/// or a math function).
pub fn parse_len(src: &str, it: &Item, a: Accept) -> Option<SpecLen> {
    let form = if it.kind() == Kind::Percentage {
        Form::Pct
    } else {
        Form::Len
    };
    let v = match it.kind() {
        Kind::Dimension => unit(src, it)?,
        Kind::Percentage if a.pct => Linear {
            pct: it.tok.number(src) as f32,
            ..Linear::default()
        },
        Kind::Number if it.tok.number(src) == 0.0 => Linear::default(),
        Kind::Function => return super::calc::math(src, it, a),
        _ => return None,
    };
    if !a.neg && v.negative() {
        return None;
    }
    Some(SpecLen::Lin { v, form })
}

/// A plain px value, for keywords such as `thin`.
pub fn px(v: f32) -> SpecLen {
    SpecLen::Lin {
        v: Linear::px(v),
        form: Form::Len,
    }
}

/// `0%`.
pub fn zero_pct() -> SpecLen {
    SpecLen::Lin {
        v: Linear::default(),
        form: Form::Pct,
    }
}

#[cfg(test)]
mod tests {
    use super::to_fixed;

    #[test]
    fn large_lengths_left_just_under_a_unit_keep_it() {
        // a length computed in f32 can land an ulp under the multiple of 1/64 px it stands for, which past
        // 256 px is more than 1/1000 of a unit; checked below 2^20 units (16384 px), where an f32 still
        // tells units apart by many ulps
        for n in (1..130).map(|k| k * 7919) {
            let under = f32::from_bits((n as f32 / 64.0).to_bits() - 1);
            assert_eq!(to_fixed(under), n, "{under}");
            assert_eq!(to_fixed(-under), -n, "{under}");
            assert_eq!(to_fixed(n as f32 / 64.0), n);
        }
        assert_eq!(to_fixed(426.66624), 27306);
    }
}
