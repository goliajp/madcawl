//! The `content` and `aspect-ratio` properties: their declared values, computed values and serialization.

use std::fmt::Write;
use std::sync::Arc;

use crate::tokenizer::Kind;

use super::props::Specified;
use super::vals::Item;

/// One piece of a `content` list.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ContentItem {
    Text(Arc<str>),
    /// `attr()`: the originating element's attribute of that name, or nothing
    Attr(Arc<str>),
    /// `counter()` and `counters()`: the counter's name
    Counter(Arc<str>),
    OpenQuote,
    CloseQuote,
    NoOpenQuote,
    NoCloseQuote,
    /// An image, which takes no room while it is not loaded
    Image,
}

/// The computed `content`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Content {
    Normal,
    None,
    Items(Arc<[ContentItem]>),
}

/// The computed `aspect-ratio`: `auto`, a ratio, or both, the ratio's width and height as `f32` bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AspectRatio {
    pub auto: bool,
    pub ratio: Option<(u32, u32)>,
}

impl AspectRatio {
    pub const AUTO: AspectRatio = AspectRatio {
        auto: true,
        ratio: None,
    };

    /// Width over height, when a ratio is given.
    pub fn ratio(self) -> Option<f32> {
        self.ratio
            .map(|(w, h)| f32::from_bits(w) / f32::from_bits(h))
    }
}

fn text(src: &str, x: &Item) -> String {
    let mut s = String::new();
    x.tok.write_value(src, &mut s);
    s
}

fn content_item(src: &str, x: &Item) -> Option<ContentItem> {
    Some(match x.kind() {
        Kind::String => ContentItem::Text(text(src, x).into()),
        Kind::Url => ContentItem::Image,
        Kind::Function => {
            let inner = super::vals::items(src, x.inner, 8)?;
            let name = || {
                let [n, ..] = &inner[..] else { return None };
                (n.kind() == Kind::Ident).then(|| text(src, n).into())
            };
            if x.is_function(src, "attr") {
                ContentItem::Attr(name()?)
            } else if x.is_function(src, "counter") || x.is_function(src, "counters") {
                ContentItem::Counter(name()?)
            } else if ["url", "image-set", "-webkit-image-set", "linear-gradient"]
                .iter()
                .any(|f| x.is_function(src, f))
            {
                ContentItem::Image
            } else {
                return None;
            }
        }
        _ => match x.ident(src)?.as_str() {
            "open-quote" => ContentItem::OpenQuote,
            "close-quote" => ContentItem::CloseQuote,
            "no-open-quote" => ContentItem::NoOpenQuote,
            "no-close-quote" => ContentItem::NoCloseQuote,
            _ => return None,
        },
    })
}

/// The declared `content`: `normal`, `none`, or a list, any alternative text after a `/` dropped.
pub(crate) fn parse_content(src: &str, xs: &[Item]) -> Option<Specified> {
    if let [x] = xs {
        match x.ident(src).as_deref() {
            Some("normal") => return Some(Specified::Content(Content::Normal)),
            Some("none") => return Some(Specified::Content(Content::None)),
            _ => {}
        }
    }
    let list = xs.split(|x| x.is_delim(b'/')).next()?;
    if list.is_empty() {
        return None;
    }
    let items: Option<Vec<ContentItem>> = list.iter().map(|x| content_item(src, x)).collect();
    Some(Specified::Content(Content::Items(items?.into())))
}

fn positive(src: &str, x: &Item) -> Option<f32> {
    let v = (x.kind() == Kind::Number).then(|| x.tok.number(src) as f32)?;
    (v >= 0.0 && v.is_finite()).then_some(v)
}

/// The declared `aspect-ratio`: `auto`, `<number> [/ <number>]`, or both in either order.
pub(crate) fn parse_aspect_ratio(src: &str, xs: &[Item]) -> Option<Specified> {
    let auto_at = xs
        .iter()
        .position(|x| x.ident(src).as_deref() == Some("auto"));
    let rest: Vec<&Item> = match auto_at {
        Some(0) => xs[1..].iter().collect(),
        Some(k) if k == xs.len() - 1 => xs[..k].iter().collect(),
        Some(_) => return None,
        None => xs.iter().collect(),
    };
    let ratio = match rest[..] {
        [] if auto_at.is_some() => None,
        [w] => Some((positive(src, w)?, 1.0)),
        [w, s, h] if s.is_delim(b'/') => Some((positive(src, w)?, positive(src, h)?)),
        _ => return None,
    };
    // a degenerate ratio is as if `auto`
    let ratio = ratio
        .filter(|&(w, h)| w > 0.0 && h > 0.0)
        .map(|(w, h)| (w.to_bits(), h.to_bits()));
    let auto = auto_at.is_some() || ratio.is_none();
    Some(Specified::AspectRatio(AspectRatio { auto, ratio }))
}

fn quoted(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\a "),
            _ => out.push(c),
        }
    }
    out.push('"');
}

pub(crate) fn serialize_content(c: &Content, out: &mut String) {
    let items = match c {
        Content::Normal => return out.push_str("normal"),
        Content::None => return out.push_str("none"),
        Content::Items(items) => items,
    };
    for (k, it) in items.iter().enumerate() {
        if k > 0 {
            out.push(' ');
        }
        match it {
            ContentItem::Text(t) => quoted(t, out),
            ContentItem::Attr(a) => {
                let _ = write!(out, "attr({a})");
            }
            ContentItem::Counter(n) => {
                let _ = write!(out, "counter({n})");
            }
            ContentItem::OpenQuote => out.push_str("open-quote"),
            ContentItem::CloseQuote => out.push_str("close-quote"),
            ContentItem::NoOpenQuote => out.push_str("no-open-quote"),
            ContentItem::NoCloseQuote => out.push_str("no-close-quote"),
            ContentItem::Image => out.push_str("url()"),
        }
    }
}

pub(crate) fn serialize_aspect_ratio(a: AspectRatio, out: &mut String) {
    if a.auto {
        out.push_str("auto");
    }
    if let Some((w, h)) = a.ratio {
        if a.auto {
            out.push(' ');
        }
        super::cssom::number(f64::from(f32::from_bits(w)), out);
        out.push_str(" / ");
        super::cssom::number(f64::from(f32::from_bits(h)), out);
    }
}
