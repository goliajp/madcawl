//! The values of the shorthands whose parts may come in any order.

use crate::tokenizer::Kind;

use super::super::kw::{
    BorderStyle, FlexDirection, FlexWrap, ListStylePosition, ListStyleType, SizeKw, TextWrapMode,
};
use super::super::length::{px, zero_pct};
use super::super::longhand::{border_width, color};
use super::super::props::{Prop, SpecColor, Specified};
use super::super::vals::Item;
use super::one;

/// Width, style and color in any order, each at most once.
pub(super) fn wsc(src: &str, xs: &[Item]) -> Option<[Specified; 3]> {
    if xs.is_empty() || xs.len() > 3 {
        return None;
    }
    let (mut w, mut s, mut c) = (None, None, None);
    for x in xs {
        let kw = x.ident(src);
        if w.is_none()
            && let Some(v) = border_width(src, x, kw.as_deref())
        {
            w = Some(v);
        } else if s.is_none()
            && let Some(v) = kw.as_deref().and_then(BorderStyle::from_keyword)
        {
            s = Some(Specified::BorderStyle(v));
        } else if c.is_none()
            && let Some(v) = color(src, x)
        {
            c = Some(Specified::Color(v));
        } else {
            return None;
        }
    }
    Some([
        w.unwrap_or(Specified::Len(px(3.0))),
        s.unwrap_or(Specified::BorderStyle(BorderStyle::None)),
        c.unwrap_or(Specified::Color(SpecColor::Current)),
    ])
}

/// The keywords of a background layer that are not its color.
const LAYER_WORDS: &[&str] = &[
    "none",
    "repeat",
    "no-repeat",
    "repeat-x",
    "repeat-y",
    "space",
    "round",
    "center",
    "top",
    "bottom",
    "left",
    "right",
    "scroll",
    "fixed",
    "local",
    "border-box",
    "padding-box",
    "content-box",
    "text",
    "cover",
    "contain",
    "auto",
];

/// The color of the last layer, transparent when it has none.
pub(super) fn background(src: &str, xs: &[Item]) -> Option<Specified> {
    let layers: Vec<&[Item]> = xs.split(|i| i.is_comma()).collect();
    let mut found = None;
    for (n, layer) in layers.iter().enumerate() {
        let last = n + 1 == layers.len();
        for x in *layer {
            let other = match x.kind() {
                Kind::Url | Kind::Number | Kind::Percentage | Kind::Dimension => true,
                Kind::Delim(b'/') => true,
                Kind::Ident => x
                    .ident(src)
                    .is_some_and(|w| LAYER_WORDS.contains(&w.as_str())),
                Kind::Function => !is_color_function(src, x),
                _ => false,
            };
            if other {
                continue;
            }
            let c = color(src, x)?;
            if !last || found.is_some() {
                return None;
            }
            found = Some(c);
        }
    }
    Some(Specified::Color(
        found.unwrap_or(SpecColor::Rgba([0, 0, 0, 0])),
    ))
}

fn is_color_function(src: &str, x: &Item) -> bool {
    [
        "rgb",
        "rgba",
        "hsl",
        "hsla",
        "hwb",
        "lab",
        "lch",
        "oklab",
        "oklch",
        "color",
        "light-dark",
        "device-cmyk",
    ]
    .iter()
    .any(|f| x.is_function(src, f))
}

pub(super) fn flex(src: &str, xs: &[Item]) -> Option<Vec<Specified>> {
    let n = |v: f32| Specified::Number(v);
    if let [x] = xs
        && let Some(w) = x.ident(src)
    {
        return match w.as_str() {
            "none" => Some(vec![n(0.0), n(0.0), Specified::Size(SizeKw::Auto)]),
            "auto" => Some(vec![n(1.0), n(1.0), Specified::Size(SizeKw::Auto)]),
            _ => Some(vec![n(1.0), n(1.0), one(Prop::FlexBasis, src, x)?]),
        };
    }
    let mut nums = Vec::new();
    let mut basis = None;
    for x in xs {
        // a number is a factor, `flex: 0` included, never a zero basis
        let factor = x.kind() == Kind::Number && nums.len() < 2;
        if factor && (basis.is_none() || nums.is_empty()) {
            let v = x.tok.number(src) as f32;
            if v < 0.0 {
                return None;
            }
            nums.push(v);
        } else if basis.is_none() && (nums.is_empty() || nums.len() == 2 || !factor) {
            basis = Some(one(Prop::FlexBasis, src, x)?);
        } else {
            return None;
        }
    }
    if xs.is_empty() {
        return None;
    }
    Some(vec![
        n(nums.first().copied().unwrap_or(1.0)),
        n(nums.get(1).copied().unwrap_or(1.0)),
        basis.unwrap_or(Specified::Len(zero_pct())),
    ])
}

pub(super) fn list_style(src: &str, xs: &[Item]) -> Option<Vec<Specified>> {
    let (mut ty, mut pos, mut image, mut nones) = (None, None, false, 0);
    for x in xs {
        let kw = x.ident(src);
        match kw.as_deref() {
            Some("none") => nones += 1,
            Some(w) if pos.is_none() && ListStylePosition::from_keyword(w).is_some() => {
                pos = ListStylePosition::from_keyword(w)
            }
            Some(w) if ty.is_none() && ListStyleType::from_keyword(w).is_some() => {
                ty = ListStyleType::from_keyword(w)
            }
            _ if !image && matches!(x.kind(), Kind::Url | Kind::Function) => image = true,
            _ => return None,
        }
    }
    if xs.is_empty() || nones > 2 || nones == 2 && (ty.is_some() || image) {
        return None;
    }
    if nones > 0 && ty.is_none() {
        ty = Some(ListStyleType::None);
    }
    Some(vec![
        Specified::ListStyleType(ty.unwrap_or(ListStyleType::Disc)),
        Specified::ListStylePosition(pos.unwrap_or(ListStylePosition::Outside)),
    ])
}

/// A `white-space` keyword, or the two longhands in either order.
pub(super) fn white_space(src: &str, xs: &[Item]) -> Option<Vec<Specified>> {
    use super::super::kw::{WhiteSpace, WhiteSpaceCollapse};
    if let [x] = xs
        && let Some(k) = x.ident(src).as_deref().and_then(WhiteSpace::from_keyword)
    {
        let (c, w) = k.longhands();
        return Some(vec![
            Specified::WhiteSpaceCollapse(c),
            Specified::TextWrapMode(w),
        ]);
    }
    let (mut c, mut w) = (None, None);
    for x in xs {
        let k = x.ident(src)?;
        if c.is_none()
            && let Some(v) = WhiteSpaceCollapse::from_keyword(&k)
        {
            c = Some(v);
        } else if w.is_none()
            && let Some(v) = TextWrapMode::from_keyword(&k)
        {
            w = Some(v);
        } else {
            return None;
        }
    }
    if xs.is_empty() {
        return None;
    }
    Some(vec![
        Specified::WhiteSpaceCollapse(c.unwrap_or(WhiteSpaceCollapse::Collapse)),
        Specified::TextWrapMode(w.unwrap_or(TextWrapMode::Wrap)),
    ])
}

pub(super) fn flex_flow(src: &str, xs: &[Item]) -> Option<Vec<Specified>> {
    let (mut d, mut w) = (None, None);
    for x in xs {
        if d.is_none()
            && let Some(v) = one(Prop::FlexDirection, src, x)
        {
            d = Some(v);
        } else if w.is_none()
            && let Some(v) = one(Prop::FlexWrap, src, x)
        {
            w = Some(v);
        } else {
            return None;
        }
    }
    if xs.is_empty() {
        return None;
    }
    Some(vec![
        d.unwrap_or(Specified::FlexDirection(FlexDirection::Row)),
        w.unwrap_or(Specified::FlexWrap(FlexWrap::Nowrap)),
    ])
}

pub(super) fn text_wrap(src: &str, xs: &[Item]) -> Option<Vec<Specified>> {
    let mut mode = None;
    for x in xs {
        match x.ident(src).as_deref() {
            Some(w @ ("wrap" | "nowrap")) if mode.is_none() => mode = TextWrapMode::from_keyword(w),
            Some("auto" | "balance" | "pretty" | "stable") => {}
            _ => return None,
        }
    }
    if xs.is_empty() || xs.len() > 2 {
        return None;
    }
    Some(vec![Specified::TextWrapMode(
        mode.unwrap_or(TextWrapMode::Wrap),
    )])
}
