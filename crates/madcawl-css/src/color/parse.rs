//! Parsing a `<color>` from a span. A function's arguments are read into
//! a small fixed array first, then checked against the modern syntax
//! (space separated, `none`, `/ alpha`) or the legacy one (commas).

use super::named::NAMED;
use super::{Color, ColorValue, MAX_CHANNELS, Space};
use crate::Span;
use crate::parser::Parser;
use crate::tokenizer::{Kind, Token};

#[derive(Clone, Copy)]
enum Arg {
    Number(f64),
    Percent(f64),
    /// An angle, in degrees
    Angle(f64),
    None,
    Ident(Token),
    Comma,
    Slash,
}

const MAX_ARGS: usize = 2 * MAX_CHANNELS + 2;

/// The arguments of a function whose contents are `span`; `None` when
/// one is not a number, percentage, angle, identifier, `,` or `/`.
fn arguments(src: &str, span: Span, out: &mut [Arg; MAX_ARGS]) -> Option<usize> {
    let mut p = Parser::new(src, span);
    let mut n = 0;
    loop {
        p.skip_whitespace();
        let t = p.next_token();
        let a = match t.kind {
            Kind::Eof => return Some(n),
            Kind::Number => Arg::Number(t.number(src)),
            Kind::Percentage => Arg::Percent(t.number(src)),
            Kind::Dimension => {
                let v = t.number(src);
                Arg::Angle(if t.unit_is(src, "deg") {
                    v
                } else if t.unit_is(src, "grad") {
                    v * 0.9
                } else if t.unit_is(src, "rad") {
                    v.to_degrees()
                } else if t.unit_is(src, "turn") {
                    v * 360.0
                } else {
                    return None;
                })
            }
            Kind::Ident if t.value_is(src, "none") => Arg::None,
            Kind::Ident => Arg::Ident(t),
            Kind::Comma => Arg::Comma,
            Kind::Delim(b'/') => Arg::Slash,
            _ => return None,
        };
        *out.get_mut(n)? = a;
        n += 1;
    }
}

/// `n` channels and an optional `/ alpha`, without commas.
fn modern(args: &[Arg], n: usize) -> Option<(&[Arg], Option<Arg>)> {
    if args.iter().any(|a| matches!(a, Arg::Comma)) {
        return None;
    }
    match args {
        [c @ .., Arg::Slash, alpha] if c.len() == n => Some((c, Some(*alpha))),
        c if c.len() == n && !c.iter().any(|a| matches!(a, Arg::Slash)) => Some((c, None)),
        _ => None,
    }
}

/// `n` channels and an optional alpha, separated by commas, no `none`.
fn legacy(args: &[Arg], n: usize, out: &mut [Arg; MAX_CHANNELS + 1]) -> Option<usize> {
    let mut k = 0;
    for (i, a) in args.iter().enumerate() {
        match (i % 2, a) {
            (1, Arg::Comma) => {}
            (0, Arg::Number(_) | Arg::Percent(_) | Arg::Angle(_)) if k <= n => {
                out[k] = *a;
                k += 1;
            }
            _ => return None,
        }
    }
    (args.len() % 2 == 1 && (k == n || k == n + 1)).then_some(k)
}

fn alpha(a: Option<Arg>) -> Option<Option<f64>> {
    Some(match a {
        None => Some(1.0),
        Some(Arg::Number(v)) => Some(v.clamp(0.0, 1.0)),
        Some(Arg::Percent(v)) => Some((v / 100.0).clamp(0.0, 1.0)),
        Some(Arg::None) => None,
        Some(_) => return None,
    })
}

/// A number, or a percentage times `per_percent`; `none`. The factor
/// is per percent, not per 100%, so the product is the one the
/// standard's own figures come from.
fn scaled(a: Arg, per_percent: f64) -> Option<Option<f64>> {
    match a {
        Arg::Number(v) => Some(Some(v)),
        Arg::Percent(v) => Some(Some(v * per_percent)),
        Arg::None => Some(None),
        _ => None,
    }
}

fn hue(a: Arg) -> Option<Option<f64>> {
    match a {
        Arg::Number(v) | Arg::Angle(v) => Some(Some(v)),
        Arg::None => Some(None),
        _ => None,
    }
}

fn color(space: Space, channels: &[Option<f64>], alpha: Option<f64>) -> Color {
    let mut c = [None; MAX_CHANNELS];
    c[..channels.len()].copy_from_slice(channels);
    Color {
        space,
        profile: None,
        channels: c,
        count: channels.len() as u8,
        alpha,
    }
}

const ONE_THIRD: f64 = 1.0 / 3.0;
const ONE_SIXTH: f64 = 1.0 / 6.0;
const TWO_THIRD: f64 = 2.0 / 3.0;

fn hue_part(m1: f64, m2: f64, h: f64) -> f64 {
    let h = h.rem_euclid(1.0);
    if h < ONE_SIXTH {
        m1 + (m2 - m1) * h * 6.0
    } else if h < 0.5 {
        m2
    } else if h < TWO_THIRD {
        m1 + (m2 - m1) * (TWO_THIRD - h) * 6.0
    } else {
        m1
    }
}

/// HSL to RGB, all from 0 to 1, as the standard's sample code has it.
fn hls_to_rgb(h: f64, l: f64, s: f64) -> [f64; 3] {
    if s == 0.0 {
        return [l, l, l];
    }
    let m2 = if l <= 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let m1 = 2.0 * l - m2;
    [
        hue_part(m1, m2, h + ONE_THIRD),
        hue_part(m1, m2, h),
        hue_part(m1, m2, h - ONE_THIRD),
    ]
}

fn rgb(args: &[Arg]) -> Option<Color> {
    let mut l = [Arg::None; MAX_CHANNELS + 1];
    let (chans, a): (&[Arg], Option<Arg>) = match legacy(args, 3, &mut l) {
        Some(k) => {
            let same = l[..3].iter().all(|a| matches!(a, Arg::Number(_)))
                || l[..3].iter().all(|a| matches!(a, Arg::Percent(_)));
            if !same {
                return None;
            }
            (&l[..3], (k == 4).then_some(l[3]))
        }
        None => modern(args, 3)?,
    };
    let mut c = [None; 3];
    for (o, a) in c.iter_mut().zip(chans) {
        *o = Some(scaled(*a, 2.55)?.unwrap_or(0.0).clamp(0.0, 255.0));
    }
    Some(color(Space::LegacyRgb, &c, alpha(a)?))
}

/// `hsl()` (`hwb` false) or `hwb()`.
fn hsl_hwb(args: &[Arg], hwb: bool) -> Option<Color> {
    let mut l = [Arg::None; MAX_CHANNELS + 1];
    let (chans, a): (&[Arg], Option<Arg>) = match (hwb, legacy(args, 3, &mut l)) {
        (false, Some(k)) => {
            if !matches!(l[1..3], [Arg::Percent(_), Arg::Percent(_)]) {
                return None;
            }
            (&l[..3], (k == 4).then_some(l[3]))
        }
        (true, Some(_)) => return None,
        (_, None) => modern(args, 3)?,
    };
    let h = hue(chans[0])?.unwrap_or(0.0);
    let x = scaled(chans[1], 1.0)?.unwrap_or(0.0).clamp(0.0, 100.0);
    let y = scaled(chans[2], 1.0)?.unwrap_or(0.0).clamp(0.0, 100.0);
    let out = if !hwb {
        hls_to_rgb(h / 360.0, y / 100.0, x / 100.0).map(|c| c * 255.0)
    } else if x + y >= 100.0 {
        [x / (x + y) * 255.0; 3]
    } else {
        hls_to_rgb(h / 360.0, 0.5, 1.0).map(|c| (c * (100.0 - x - y) + x) / 100.0 * 255.0)
    };
    Some(color(Space::LegacyRgb, &out.map(Some), alpha(a)?))
}

/// `lab()`, `oklab()`, `lch()` or `oklch()`: lightness, each percent
/// worth `l_pct`, then a and b, or chroma and hue, each percent `pct`.
fn lab_like(args: &[Arg], space: Space, l_pct: f64, pct: f64) -> Option<Color> {
    let (c, a) = modern(args, 3)?;
    let polar = matches!(space, Space::Lch | Space::Oklch);
    let third = if polar {
        hue(c[2])?.map(|h| h.rem_euclid(360.0))
    } else {
        scaled(c[2], pct)?
    };
    let chans = [scaled(c[0], l_pct)?, scaled(c[1], pct)?, third];
    Some(color(space, &chans, alpha(a)?))
}

fn predefined(src: &str, t: &Token) -> Option<Space> {
    const SPACES: &[(&str, Space)] = &[
        ("srgb", Space::Srgb),
        ("srgb-linear", Space::SrgbLinear),
        ("display-p3", Space::DisplayP3),
        ("a98-rgb", Space::A98Rgb),
        ("prophoto-rgb", Space::ProphotoRgb),
        ("rec2020", Space::Rec2020),
        ("xyz", Space::XyzD65),
        ("xyz-d50", Space::XyzD50),
        ("xyz-d65", Space::XyzD65),
    ];
    SPACES
        .iter()
        .find(|(n, _)| t.value_is(src, n))
        .map(|&(_, s)| s)
}

/// `color(space c1 c2 c3 [/ alpha])`, or `color(--profile c... )`.
fn color_function(src: &str, args: &[Arg]) -> Option<Color> {
    let (Arg::Ident(t), rest) = args.split_first()? else {
        return None;
    };
    let mut name = t.chars(src);
    let dashed = name.next() == Some('-') && name.next() == Some('-');
    let (space, n) = match predefined(src, t) {
        Some(s) => (s, 3),
        None if dashed => {
            let n = rest.iter().take_while(|a| !matches!(a, Arg::Slash)).count();
            (Space::Custom, n)
        }
        None => return None,
    };
    if n == 0 || n > MAX_CHANNELS {
        return None;
    }
    let (c, a) = modern(rest, n)?;
    let mut chans = [None; MAX_CHANNELS];
    for (o, a) in chans.iter_mut().zip(c) {
        *o = scaled(*a, 0.01)?;
    }
    let mut out = color(space, &chans[..n], alpha(a)?);
    if space == Space::Custom {
        let mut profile = String::new();
        t.write_value(src, &mut profile);
        out.profile = Some(profile.into());
    }
    Some(out)
}

fn device_cmyk(args: &[Arg]) -> Option<Color> {
    let mut l = [Arg::None; MAX_CHANNELS + 1];
    let (chans, a): (&[Arg], Option<Arg>) = match legacy(args, 4, &mut l) {
        Some(k) if l[..k].iter().all(|a| matches!(a, Arg::Number(_))) => {
            (&l[..4], (k == 5).then_some(l[4]))
        }
        Some(_) => return None,
        None => modern(args, 4)?,
    };
    let mut c = [None; 4];
    for (o, a) in c.iter_mut().zip(chans) {
        *o = scaled(*a, 0.01)?.map(|v| v.clamp(0.0, 1.0));
    }
    Some(color(Space::DeviceCmyk, &c, alpha(a)?))
}

fn hex(src: &str, t: &Token) -> Option<Color> {
    let mut d = [0u32; 8];
    let mut n = 0;
    for ch in t.chars(src) {
        *d.get_mut(n)? = ch.to_digit(16)?;
        n += 1;
    }
    let (c, a) = match n {
        3 | 4 => (
            [d[0] * 17, d[1] * 17, d[2] * 17],
            (n == 4).then(|| d[3] * 17),
        ),
        6 | 8 => (
            [d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5]],
            (n == 8).then(|| d[6] * 16 + d[7]),
        ),
        _ => return None,
    };
    let alpha = a.map_or(1.0, |a| a as f64 / 255.0);
    Some(color(
        Space::LegacyRgb,
        &c.map(|v| Some(v as f64)),
        Some(alpha),
    ))
}

fn named(src: &str, t: &Token) -> Option<Color> {
    let mut buf = [0u8; 24];
    let mut n = 0;
    for ch in t.chars(src) {
        if !ch.is_ascii() {
            return None;
        }
        *buf.get_mut(n)? = ch.to_ascii_lowercase() as u8;
        n += 1;
    }
    let name = std::str::from_utf8(&buf[..n]).ok()?;
    if name == "transparent" {
        return Some(color(Space::LegacyRgb, &[Some(0.0); 3], Some(0.0)));
    }
    let i = NAMED.binary_search_by(|(k, _)| k.cmp(&name)).ok()?;
    let rgb = NAMED[i].1.map(|v| Some(v as f64));
    Some(color(Space::LegacyRgb, &rgb, Some(1.0)))
}

fn function(src: &str, t: &Token, span: Span, nested: bool) -> Option<ColorValue> {
    if t.value_is(src, "light-dark") {
        if nested {
            return None;
        }
        let (a, b) = split_comma(src, span)?;
        let (ColorValue::Color(a), ColorValue::Color(b)) =
            (color_in(src, a, true)?, color_in(src, b, true)?)
        else {
            return None;
        };
        return Some(ColorValue::LightDark(Box::new((a, b))));
    }
    let mut buf = [Arg::None; MAX_ARGS];
    let n = arguments(src, span, &mut buf)?;
    let args = &buf[..n];
    let is = |name: &str| t.value_is(src, name);
    let c = if is("rgb") || is("rgba") {
        rgb(args)
    } else if is("hsl") || is("hsla") {
        hsl_hwb(args, false)
    } else if is("hwb") {
        hsl_hwb(args, true)
    } else if is("lab") {
        lab_like(args, Space::Lab, 1.0, 1.25)
    } else if is("oklab") {
        lab_like(args, Space::Oklab, 0.01, 0.004)
    } else if is("lch") {
        lab_like(args, Space::Lch, 1.0, 1.5)
    } else if is("oklch") {
        lab_like(args, Space::Oklch, 0.01, 0.004)
    } else if is("color") {
        color_function(src, args)
    } else if is("device-cmyk") {
        device_cmyk(args)
    } else {
        None
    };
    c.map(ColorValue::Color)
}

/// The two halves of `span` around its one top-level comma.
fn split_comma(src: &str, span: Span) -> Option<(Span, Span)> {
    let mut p = Parser::new(src, span);
    let mut comma = None;
    loop {
        let t = p.next_token();
        match t.kind {
            Kind::Eof => break,
            Kind::Comma if comma.is_none() => comma = Some(t.span),
            Kind::Comma => return None,
            k => {
                p.skip_block(k);
            }
        }
    }
    let c = comma?;
    Some((
        Span::new(span.start as usize, c.start as usize),
        Span::new(c.end as usize, span.end as usize),
    ))
}

/// The function token and contents of `src[span]` when it is one
/// function and nothing else, whitespace aside.
fn only_function(src: &str, span: Span) -> Option<(Token, Span)> {
    let mut p = Parser::new(src, span);
    p.skip_whitespace();
    let t = p.next_token();
    if t.kind != Kind::Function {
        return None;
    }
    let end = p.skip_block(Kind::Function);
    p.skip_whitespace();
    (p.next_token().kind == Kind::Eof).then(|| (t, Span::new(t.span.end as usize, end)))
}

/// The two arguments of a `light-dark()` that is all of `src[span]`.
pub fn light_dark_arguments(src: &str, span: Span) -> Option<(Span, Span)> {
    let (t, args) = only_function(src, span)?;
    if !t.value_is(src, "light-dark") {
        return None;
    }
    split_comma(src, args)
}

/// `src[span]` as one `<color>`, whitespace around it aside.
pub fn parse_color(src: &str, span: Span) -> Option<ColorValue> {
    color_in(src, span, false)
}

/// A `light-dark()` in a `light-dark()` is not taken, which bounds the
/// recursion.
fn color_in(src: &str, span: Span, nested: bool) -> Option<ColorValue> {
    let mut p = Parser::new(src, span);
    p.skip_whitespace();
    let t = p.peek();
    let value = match t.kind {
        Kind::Function => {
            let (t, args) = only_function(src, span)?;
            return function(src, &t, args, nested);
        }
        Kind::Ident => named(src, &t)?,
        Kind::Hash { .. } => hex(src, &t)?,
        _ => return None,
    };
    p.next_token();
    p.skip_whitespace();
    (p.next_token().kind == Kind::Eof).then_some(ColorValue::Color(value))
}
