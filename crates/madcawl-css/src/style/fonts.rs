//! The font properties' declared values: family lists, sizes, weights,
//! line heights, and the `font` shorthand.

use std::sync::Arc;

use crate::tokenizer::Kind;

use super::kw::FontStyle;
use super::length::{Accept, parse_len, px};
use super::longhand::non_negative;
use super::props::{FontSize, Specified, Weight};
use super::vals::Item;

const POSITIVE: Accept = Accept {
    pct: true,
    neg: false,
};

const GENERIC: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
    "fangsong",
    "-webkit-body",
    "-webkit-standard",
    "-apple-system",
    "-webkit-pictograph",
];

/// A family list as `getComputedStyle` writes it: names joined by `, `,
/// a name quoted unless it is a generic family or every word of it is
/// an identifier.
pub(crate) fn family(src: &str, xs: &[Item]) -> Option<Specified> {
    let mut out = String::new();
    for part in xs.split(|i| i.is_comma()) {
        if !out.is_empty() {
            out.push_str(", ");
        }
        match part {
            [s] if s.kind() == Kind::String => {
                let mut v = String::new();
                s.tok.write_value(src, &mut v);
                quote(&v, &mut out);
            }
            [] => return None,
            words => {
                let mut name = String::new();
                for w in words {
                    if w.kind() != Kind::Ident {
                        return None;
                    }
                    if !name.is_empty() {
                        name.push(' ');
                    }
                    w.tok.write_value(src, &mut name);
                }
                let lower = name.to_ascii_lowercase();
                if words.len() == 1 && GENERIC.contains(&lower.as_str()) {
                    out.push_str(&lower);
                } else if words.len() == 1
                    && matches!(lower.as_str(), "inherit" | "initial" | "unset" | "default")
                {
                    return None;
                } else if words.len() == 1 {
                    out.push_str(&name);
                } else {
                    quote(&name, &mut out);
                }
            }
        }
    }
    Some(Specified::Family(Arc::from(out)))
}

fn quote(name: &str, out: &mut String) {
    let plain = !name.is_empty()
        && !GENERIC.contains(&name)
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b >= 0x80)
        && !name.as_bytes()[0].is_ascii_digit()
        && !name.starts_with("--");
    if plain {
        out.push_str(name);
        return;
    }
    out.push('"');
    for c in name.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
}

pub(crate) const SIZE_KEYWORDS: &[&str] = &[
    "xx-small",
    "x-small",
    "small",
    "medium",
    "large",
    "x-large",
    "xx-large",
    "xxx-large",
];

pub(crate) fn font_size(src: &str, x: &Item, kw: Option<&str>) -> Option<Specified> {
    Some(Specified::FontSize(match kw {
        Some("smaller") => FontSize::Smaller,
        Some("larger") => FontSize::Larger,
        Some(w) => FontSize::Keyword(SIZE_KEYWORDS.iter().position(|k| *k == w)? as u8),
        None => FontSize::Len(parse_len(src, x, POSITIVE)?),
    }))
}

pub(crate) fn weight(src: &str, x: &Item, kw: Option<&str>) -> Option<Weight> {
    Some(match kw {
        Some("normal") => Weight::Abs(400),
        Some("bold") => Weight::Abs(700),
        Some("bolder") => Weight::Bolder,
        Some("lighter") => Weight::Lighter,
        Some(_) => return None,
        None if x.kind() == Kind::Number => {
            let v = x.tok.number(src);
            if !(1.0..=1000.0).contains(&v) {
                return None;
            }
            Weight::Abs(v as u16)
        }
        None => return None,
    })
}

pub(crate) fn line_height(src: &str, x: &Item, kw: Option<&str>) -> Option<Specified> {
    Some(match kw {
        Some("normal") => Specified::Normal,
        Some(_) => return None,
        None if x.kind() == Kind::Number => Specified::Number(non_negative(src, x)?),
        None => Specified::Len(parse_len(src, x, POSITIVE)?),
    })
}

/// The font size and family `font` sets for a system font keyword.
fn system_font(w: &str) -> Option<Vec<Specified>> {
    let size = match w {
        "caption" | "icon" | "menu" | "message-box" | "status-bar" => 13.0,
        "small-caption" => 11.0,
        "-webkit-small-control" => 11.0,
        "-webkit-control" | "-webkit-mini-control" => 13.0,
        _ if w.starts_with("-apple-system-") => 13.0,
        _ => return None,
    };
    Some(vec![
        Specified::FontStyle(FontStyle::Normal),
        Specified::Weight(Weight::Abs(400)),
        Specified::FontSize(FontSize::Len(px(size))),
        Specified::Normal,
        Specified::Family("system-ui".into()),
    ])
}

pub(crate) fn font_shorthand(src: &str, xs: &[Item]) -> Option<Vec<Specified>> {
    if let [x] = xs
        && let Some(w) = x.ident(src)
    {
        return system_font(&w);
    }
    let (mut style, mut wt) = (None, None);
    let mut i = 0;
    while let Some(x) = xs.get(i) {
        let kw = x.ident(src);
        match kw.as_deref() {
            Some("normal" | "small-caps") => {}
            Some(k @ ("italic" | "oblique")) if style.is_none() => {
                style = FontStyle::from_keyword(k)
            }
            Some(
                "ultra-condensed" | "extra-condensed" | "condensed" | "semi-condensed"
                | "semi-expanded" | "expanded" | "extra-expanded" | "ultra-expanded",
            ) => {}
            _ if wt.is_none() && x.kind() != Kind::Dimension && x.kind() != Kind::Percentage => {
                match weight(src, x, kw.as_deref()) {
                    Some(w) => wt = Some(w),
                    None => break,
                }
            }
            _ => break,
        }
        i += 1;
    }
    let size = xs.get(i)?;
    let size = font_size(src, size, size.ident(src).as_deref())?;
    i += 1;
    let mut lh = Specified::Normal;
    if xs.get(i).is_some_and(|x| x.is_delim(b'/')) {
        let x = xs.get(i + 1)?;
        lh = line_height(src, x, x.ident(src).as_deref())?;
        i += 2;
    }
    let fam = family(src, xs.get(i..).filter(|r| !r.is_empty())?)?;
    Some(vec![
        Specified::FontStyle(style.unwrap_or(FontStyle::Normal)),
        Specified::Weight(wt.unwrap_or(Weight::Abs(400))),
        size,
        lh,
        fam,
    ])
}
