//! `calc()`, `min()`, `max()` and `clamp()` of lengths and
//! percentages, parsed to a tree of sums and scalings.

use std::sync::Arc;

use crate::Span;
use crate::tokenizer::{Kind, Token};

use super::device::Device;
use super::length::{Accept, Ctx, Expr, Form, Linear, SpecLen, parse_len, unit};
use super::vals::{Item, items};

pub(crate) fn math(src: &str, it: &Item, a: Accept) -> Option<SpecLen> {
    let mut depth = 0;
    let e = function(src, it, a, &mut depth)?;
    let Term::Len(e) = e else { return None };
    Some(match e {
        Expr::Leaf(v) => SpecLen::Lin {
            v,
            form: Form::Calc,
        },
        e => SpecLen::Expr(Arc::new(e)),
    })
}

/// A term of a math expression: a plain number or a length.
enum Term {
    Num(f32),
    Len(Expr),
}

const MAX_DEPTH: u32 = 32;

fn function(src: &str, it: &Item, a: Accept, depth: &mut u32) -> Option<Term> {
    *depth += 1;
    if *depth > MAX_DEPTH {
        return None;
    }
    let args = items(src, it.inner, 256)?;
    let r = if it.is_function(src, "calc") || it.is_function(src, "-webkit-calc") {
        sum(src, &args, a, depth)
    } else if it.is_function(src, "min") || it.is_function(src, "max") {
        let mut v = Vec::new();
        for part in args.split(|i| i.is_comma()) {
            match sum(src, part, a, depth)? {
                Term::Len(e) => v.push(e),
                Term::Num(_) => return None,
            }
        }
        if v.is_empty() {
            return None;
        }
        Some(Term::Len(if it.is_function(src, "min") {
            Expr::Min(v)
        } else {
            Expr::Max(v)
        }))
    } else if it.is_function(src, "clamp") {
        let mut v = Vec::new();
        for part in args.split(|i| i.is_comma()) {
            match sum(src, part, a, depth)? {
                Term::Len(e) => v.push(e),
                Term::Num(_) => return None,
            }
        }
        let [lo, mid, hi]: [Expr; 3] = v.try_into().ok()?;
        Some(Term::Len(Expr::Max(vec![lo, Expr::Min(vec![mid, hi])])))
    } else {
        None
    };
    *depth -= 1;
    r
}

fn sum(src: &str, xs: &[Item], a: Accept, depth: &mut u32) -> Option<Term> {
    let mut parts = xs.split_inclusive(|i| i.is_delim(b'+') || i.is_delim(b'-'));
    let mut acc: Option<Term> = None;
    let mut sign = 1.0;
    for part in parts.by_ref() {
        let (operand, op) = match part.split_last() {
            Some((l, rest)) if l.is_delim(b'+') || l.is_delim(b'-') => {
                (rest, Some(if l.is_delim(b'-') { -1.0 } else { 1.0 }))
            }
            _ => (part, None),
        };
        let t = product(src, operand, a, depth)?;
        let t = match t {
            Term::Num(n) => Term::Num(n * sign),
            Term::Len(e) => Term::Len(scale(e, sign)),
        };
        acc = Some(match (acc, t) {
            (None, t) => t,
            (Some(Term::Num(x)), Term::Num(y)) => Term::Num(x + y),
            (Some(Term::Len(x)), Term::Len(y)) => Term::Len(add(x, y)),
            _ => return None,
        });
        sign = op.unwrap_or(1.0);
    }
    acc
}

fn product(src: &str, xs: &[Item], a: Accept, depth: &mut u32) -> Option<Term> {
    let (first, mut rest) = xs.split_first()?;
    let mut acc = value(src, first, a, depth)?;
    while let Some((op, r)) = rest.split_first() {
        let (x, r) = r.split_first()?;
        rest = r;
        let x = value(src, x, a, depth)?;
        acc = match (acc, x, op.is_delim(b'*'), op.is_delim(b'/')) {
            (Term::Num(p), Term::Num(q), true, _) => Term::Num(p * q),
            (Term::Num(p), Term::Num(q), _, true) if q != 0.0 => Term::Num(p / q),
            (Term::Len(e), Term::Num(q), true, _) | (Term::Num(q), Term::Len(e), true, _) => {
                Term::Len(scale(e, q))
            }
            (Term::Len(e), Term::Num(q), _, true) if q != 0.0 => Term::Len(scale(e, 1.0 / q)),
            _ => return None,
        };
    }
    Some(acc)
}

fn value(src: &str, it: &Item, a: Accept, depth: &mut u32) -> Option<Term> {
    match it.kind() {
        Kind::Number => Some(Term::Num(it.tok.number(src) as f32)),
        Kind::Dimension => Some(Term::Len(Expr::Leaf(unit(src, it)?))),
        Kind::Percentage if a.pct => Some(Term::Len(Expr::Leaf(Linear {
            pct: it.tok.number(src) as f32,
            ..Linear::default()
        }))),
        Kind::OpenParen => sum(src, &items(src, it.inner, 256)?, a, depth),
        Kind::Function => function(src, it, a, depth),
        _ => None,
    }
}

fn scale(e: Expr, k: f32) -> Expr {
    match e {
        Expr::Leaf(l) => Expr::Leaf(l.scale(k)),
        e if k == 1.0 => e,
        e => Expr::Scale(Box::new(e), k),
    }
}

fn add(a: Expr, b: Expr) -> Expr {
    match (a, b) {
        (Expr::Leaf(x), Expr::Leaf(y)) => Expr::Leaf(x.zip(y, |p, q| p + q)),
        (a, b) => Expr::Sum(Box::new(a), Box::new(b)),
    }
}

/// A math function of lengths in a media query, `t` its function token
/// and `inner` its arguments; font-relative units are of the initial
/// font size.
pub(crate) fn media_length(src: &str, t: Token, inner: Span, d: &Device) -> Option<f64> {
    let a = Accept {
        pct: false,
        neg: true,
    };
    let c = Ctx {
        em: 16.0,
        rem: 16.0,
        vw: d.width as f32,
        vh: d.height as f32,
    };
    let (px, _) = parse_len(src, &Item { tok: t, inner }, a)?.resolve_float(&c)?;
    Some(f64::from(px))
}
