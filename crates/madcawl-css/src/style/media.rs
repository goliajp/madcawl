//! Media queries (Media Queries Level 4) evaluated against one device.
//! Logic is three-valued as the standard says: a feature this engine
//! does not know, or a value it cannot read, is unknown, and a query
//! that comes out unknown does not match.

use crate::Span;
use crate::tokenizer::{Kind, Token};

use super::device::Device;
use super::tokens;

/// Whether the media query list `src[span]` matches; an empty list does.
pub fn matches_media(src: &str, span: Span, device: &Device) -> bool {
    let toks = tokens(src, span);
    if toks.is_empty() {
        return true;
    }
    let mut depth = 0u32;
    let top_comma = |t: &Token| {
        match t.kind {
            Kind::Function | Kind::OpenParen | Kind::OpenSquare | Kind::OpenCurly => depth += 1,
            Kind::CloseParen | Kind::CloseSquare | Kind::CloseCurly => {
                depth = depth.saturating_sub(1)
            }
            _ => {}
        }
        t.kind == Kind::Comma && depth == 0
    };
    toks.split(top_comma).any(|q| {
        let mut e = Eval {
            src,
            t: q,
            i: 0,
            d: device,
        };
        e.query() == Some(true) && e.i == q.len()
    })
}

struct Eval<'a> {
    src: &'a str,
    t: &'a [Token],
    i: usize,
    d: &'a Device,
}

#[derive(Clone, Debug)]
enum Value {
    Num(f64),
    /// CSS pixels
    Len(f64),
    Ratio(f64),
    /// dppx
    Res(f64),
    Ident(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
}

impl Op {
    fn flip(self) -> Op {
        match self {
            Op::Lt => Op::Gt,
            Op::Le => Op::Ge,
            Op::Gt => Op::Lt,
            Op::Ge => Op::Le,
            Op::Eq => Op::Eq,
        }
    }

    fn holds(self, a: f64, b: f64) -> bool {
        match self {
            Op::Lt => a < b,
            Op::Le => a <= b,
            Op::Eq => (a - b).abs() < 1e-9,
            Op::Ge => a >= b,
            Op::Gt => a > b,
        }
    }
}

fn and(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}

fn or(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

impl Eval<'_> {
    fn peek(&self) -> Option<&Token> {
        self.t.get(self.i)
    }

    fn ident(&self, name: &str) -> bool {
        self.peek()
            .is_some_and(|t| t.kind == Kind::Ident && t.value_is(self.src, name))
    }

    fn eat(&mut self, kind: Kind) -> bool {
        let hit = self.peek().is_some_and(|t| t.kind == kind);
        self.i += hit as usize;
        hit
    }

    fn query(&mut self) -> Option<bool> {
        if self.peek()?.kind != Kind::Ident || self.ident("not") && self.next_is_paren() {
            return self.condition(true);
        }
        let not = self.ident("not");
        self.i += not as usize;
        if !not && self.ident("only") {
            self.i += 1;
        }
        let t = *self.peek()?;
        if t.kind != Kind::Ident {
            return None;
        }
        self.i += 1;
        let ty = match () {
            _ if t.value_is(self.src, "all") => true,
            _ if t.value_is(self.src, "screen") => !self.d.print,
            _ if t.value_is(self.src, "print") => self.d.print,
            _ => false,
        };
        let mut r = Some(ty);
        if self.ident("and") {
            self.i += 1;
            r = and(r, self.condition(false));
        }
        if not { r.map(|b| !b) } else { r }
    }

    fn next_is_paren(&self) -> bool {
        self.t
            .get(self.i + 1)
            .is_some_and(|t| t.kind == Kind::OpenParen)
    }

    /// `not <in-parens>`, or in-parens joined by `and`, or by `or`
    /// when `or` may appear.
    fn condition(&mut self, may_or: bool) -> Option<bool> {
        if self.ident("not") {
            self.i += 1;
            return self.in_parens().map(|b| !b);
        }
        let mut r = self.in_parens();
        let mut joiner: Option<bool> = None;
        loop {
            let is_and = self.ident("and");
            let is_or = may_or && self.ident("or");
            if !is_and && !is_or || joiner.is_some_and(|j| j != is_and) {
                return r;
            }
            joiner = Some(is_and);
            self.i += 1;
            let next = self.in_parens();
            r = if is_and { and(r, next) } else { or(r, next) };
        }
    }

    /// The tokens up to the parenthesis that closes the one at `open`.
    fn close_of(&self, open: usize) -> usize {
        let mut depth = 0;
        for (k, t) in self.t.iter().enumerate().skip(open) {
            match t.kind {
                Kind::OpenParen | Kind::Function => depth += 1,
                Kind::CloseParen => {
                    depth -= 1;
                    if depth == 0 {
                        return k;
                    }
                }
                _ => {}
            }
        }
        self.t.len()
    }

    fn in_parens(&mut self) -> Option<bool> {
        let t = *self.peek()?;
        let close = self.close_of(self.i);
        let inner = &self.t[(self.i + 1).min(close)..close];
        self.i = (close + 1).min(self.t.len());
        if t.kind != Kind::OpenParen {
            // general-enclosed
            return None;
        }
        let first = inner.first()?;
        let nested = first.kind == Kind::OpenParen
            || first.kind == Kind::Ident && first.value_is(self.src, "not");
        let mut sub = Eval {
            src: self.src,
            t: inner,
            i: 0,
            d: self.d,
        };
        let r = if nested {
            sub.condition(true)
        } else {
            sub.feature()
        };
        if sub.i == inner.len() { r } else { None }
    }

    fn op(&mut self) -> Option<Op> {
        let d = |t: Option<&Token>| match t.map(|t| t.kind) {
            Some(Kind::Delim(c)) => Some(c),
            _ => None,
        };
        let c = d(self.peek())?;
        let eq = d(self.t.get(self.i + 1)) == Some(b'=');
        let (op, n) = match (c, eq) {
            (b'<', true) => (Op::Le, 2),
            (b'>', true) => (Op::Ge, 2),
            (b'<', false) => (Op::Lt, 1),
            (b'>', false) => (Op::Gt, 1),
            (b'=', _) => (Op::Eq, 1),
            _ => return None,
        };
        self.i += n;
        Some(op)
    }

    fn value(&mut self) -> Option<Value> {
        let t = *self.peek()?;
        self.i += 1;
        let src = self.src;
        Some(match t.kind {
            Kind::Number => {
                let n = t.number(src);
                let slash = matches!(self.peek().map(|t| t.kind), Some(Kind::Delim(b'/')));
                if slash
                    && self
                        .t
                        .get(self.i + 1)
                        .is_some_and(|t| t.kind == Kind::Number)
                {
                    let d = self.t[self.i + 1].number(src);
                    self.i += 2;
                    Value::Ratio(n / d)
                } else {
                    Value::Num(n)
                }
            }
            Kind::Dimension => {
                let n = t.number(src);
                let mut unit = String::new();
                unit.extend(t.unit_chars(src));
                unit.make_ascii_lowercase();
                match length_px(n, &unit, self.d) {
                    Some(px) => Value::Len(px),
                    None => Value::Res(match unit.as_str() {
                        "dppx" | "x" => n,
                        "dpi" => n / 96.0,
                        "dpcm" => n * 2.54 / 96.0,
                        _ => return None,
                    }),
                }
            }
            Kind::Ident => {
                let mut s = String::new();
                t.write_value(src, &mut s);
                s.make_ascii_lowercase();
                Value::Ident(s)
            }
            _ => return None,
        })
    }

    fn feature(&mut self) -> Option<bool> {
        let start = self.i;
        // `name`, `name: value`, `name op value`
        if let Some(t) = self.peek().copied()
            && t.kind == Kind::Ident
        {
            let mut name = String::new();
            t.write_value(self.src, &mut name);
            name.make_ascii_lowercase();
            self.i += 1;
            if self.i == self.t.len() {
                return self.boolean(&name);
            }
            if self.eat(Kind::Colon) {
                let v = self.value()?;
                return self.plain(&name, &v);
            }
            if let Some(op) = self.op() {
                let v = self.value()?;
                return self.range(&name, op, &v);
            }
            self.i = start;
        }
        // `value op name [op value]`
        let a = self.value()?;
        let op1 = self.op()?;
        let t = *self.peek()?;
        if t.kind != Kind::Ident {
            return None;
        }
        self.i += 1;
        let mut name = String::new();
        t.write_value(self.src, &mut name);
        name.make_ascii_lowercase();
        let r = self.range(&name, op1.flip(), &a);
        if self.i == self.t.len() {
            return r;
        }
        let op2 = self.op()?;
        let b = self.value()?;
        and(r, self.range(&name, op2, &b))
    }

    fn plain(&self, name: &str, v: &Value) -> Option<bool> {
        if let Some(n) = name.strip_prefix("min-") {
            return self.range(n, Op::Ge, v);
        }
        if let Some(n) = name.strip_prefix("max-") {
            return self.range(n, Op::Le, v);
        }
        if let Some(n) = name.strip_prefix("-webkit-min-") {
            return self.range(&format!("-webkit-{n}"), Op::Ge, v);
        }
        if let Some(n) = name.strip_prefix("-webkit-max-") {
            return self.range(&format!("-webkit-{n}"), Op::Le, v);
        }
        match v {
            Value::Ident(s) => Some(self.discrete(name)? == s.as_str()),
            _ => self.range(name, Op::Eq, v),
        }
    }

    fn boolean(&self, name: &str) -> Option<bool> {
        if let Some(n) = self.numeric(name) {
            return Some(n != 0.0);
        }
        let v = self.discrete(name)?;
        Some(!matches!(v, "none" | "no-preference"))
    }

    /// A feature compared as a number, in the unit `Value::to` gives.
    fn numeric(&self, name: &str) -> Option<f64> {
        let d = self.d;
        Some(match name {
            "width" | "device-width" => d.width,
            "height" | "device-height" => d.height,
            "aspect-ratio" | "device-aspect-ratio" => d.width / d.height,
            "resolution" | "-webkit-device-pixel-ratio" => d.scale,
            "color" => 8.0,
            "color-index" | "monochrome" | "grid" => 0.0,
            _ => return None,
        })
    }

    fn discrete(&self, name: &str) -> Option<&'static str> {
        let d = self.d;
        Some(match name {
            "orientation" => {
                if d.height >= d.width {
                    "portrait"
                } else {
                    "landscape"
                }
            }
            "prefers-color-scheme" => {
                if d.dark {
                    "dark"
                } else {
                    "light"
                }
            }
            "prefers-reduced-motion"
            | "prefers-contrast"
            | "prefers-reduced-transparency"
            | "prefers-reduced-data" => "no-preference",
            "forced-colors" | "inverted-colors" => "none",
            "hover" | "any-hover" => "hover",
            "pointer" | "any-pointer" => "fine",
            "color-gamut" => "srgb",
            "display-mode" => "browser",
            "dynamic-range" | "video-dynamic-range" => "standard",
            "update" => "fast",
            "overflow-block" => "scroll",
            "overflow-inline" => "scroll",
            "scan" => "progressive",
            "scripting" => {
                if d.scripting {
                    "enabled"
                } else {
                    "none"
                }
            }
            _ => return None,
        })
    }

    fn range(&self, name: &str, op: Op, v: &Value) -> Option<bool> {
        if name == "color-gamut" {
            return None;
        }
        let have = self.numeric(name)?;
        let want = match (name, v) {
            ("width" | "height" | "device-width" | "device-height", Value::Len(px)) => *px,
            ("width" | "height" | "device-width" | "device-height", Value::Num(n)) if *n == 0.0 => {
                0.0
            }
            ("aspect-ratio" | "device-aspect-ratio", Value::Ratio(r) | Value::Num(r)) => *r,
            ("resolution", Value::Res(r)) => *r,
            (
                "-webkit-device-pixel-ratio" | "color" | "color-index" | "monochrome" | "grid",
                Value::Num(n),
            ) => *n,
            _ => return None,
        };
        Some(op.holds(have, want))
    }
}

/// A length in CSS pixels, for media queries and computed values: font
/// relative units against the initial font size.
pub(crate) fn length_px(n: f64, unit: &str, d: &Device) -> Option<f64> {
    Some(match unit {
        "px" => n,
        "em" | "rem" => n * 16.0,
        "ex" | "rex" => n * 8.0,
        "ch" | "rch" => n * 8.0,
        "vw" | "svw" | "lvw" | "dvw" => n * d.width / 100.0,
        "vh" | "svh" | "lvh" | "dvh" => n * d.height / 100.0,
        "vmin" => n * d.width.min(d.height) / 100.0,
        "vmax" => n * d.width.max(d.height) / 100.0,
        "cm" => n * 96.0 / 2.54,
        "mm" => n * 96.0 / 25.4,
        "q" => n * 96.0 / 101.6,
        "in" => n * 96.0,
        "pt" => n * 96.0 / 72.0,
        "pc" => n * 16.0,
        _ => return None,
    })
}
