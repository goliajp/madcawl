//! Replaced elements: their natural sizes, which this crate does not load, are what WebKit gives them when
//! nothing loads, and they are sized from those and their computed values.

use madcawl_css::style::{ComputedStyle, Size as CssSize, SizeKw};
use madcawl_dom::NodeId;
use madcawl_html::Namespace;
use madcawl_str::atom;
use madcawl_text::Style;

use crate::au::{Au, Size};
use crate::cx::{Cx, Laid, Space, Strut};
use crate::fragment::{FragKind, Fragment};
use crate::sizing::margins_for;
use crate::style::{Edges, content_size, size};

/// A natural width and height, either of which may be missing, and a natural aspect ratio (width over
/// height).
#[derive(Clone, Copy, Debug, Default)]
pub struct Natural {
    pub w: Option<Au>,
    pub h: Option<Au>,
    pub ratio: Option<f32>,
}

impl Natural {
    fn sized(w: f32, h: f32) -> Natural {
        Natural {
            w: Some(Au::from_px(w)),
            h: Some(Au::from_px(h)),
            ratio: (h > 0.0).then(|| w / h),
        }
    }
}

fn named(cx: &Cx, n: NodeId) -> Option<madcawl_str::Atom> {
    (cx.dom.namespace(n) == Some(Namespace::Html))
        .then(|| cx.dom.local_name(n))
        .flatten()
}

fn attr(cx: &Cx, n: NodeId, a: madcawl_str::Atom) -> Option<String> {
    let mut b = Vec::new();
    cx.dom.attribute(n, a)?.write_wtf8(&mut b);
    Some(String::from_utf8_lossy(&b).into_owned())
}

// a non-negative dimension attribute as HTML reads one: its leading digits and fraction, not a percentage
fn dimension(v: &str) -> Option<f32> {
    let v = v.trim_start_matches([' ', '\t', '\n', '\x0c', '\r']);
    let end = v
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' && i > 0))
        .map_or(v.len(), |(i, _)| i);
    if v[end..].starts_with('%') {
        return None;
    }
    v[..end].parse::<f32>().ok().filter(|x| x.is_finite())
}

// an `svg` root's ratio from its `viewBox`
fn view_box(v: &str) -> Option<f32> {
    let n: Vec<f32> = v
        .split([' ', ',', '\t', '\n', '\r'])
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f32>().ok())
        .collect::<Option<_>>()?;
    match n[..] {
        [_, _, w, h] if w > 0.0 && h > 0.0 => Some(w / h),
        _ => None,
    }
}

// a broken image with alternative text shows the text in a box of its width and the font's line height,
// each rounded up and with 2px on every side
fn alt_box(cx: &mut Cx, b: u32, alt: &str) -> Natural {
    let f = cx.font(b);
    let units: Vec<u16> = alt.encode_utf16().collect();
    let style = Style {
        face: f.face,
        size: f.size,
        features: 0,
    };
    let w = cx.shaper.shape_word(&units, &style, false).advance;
    Natural::sized(w.ceil() + 4.0, f.normal_line_height().px().ceil() + 4.0)
}

fn img(cx: &mut Cx, b: u32, n: NodeId) -> Natural {
    if let Some(alt) = attr(cx, n, atom!("alt")).filter(|a| !a.is_empty()) {
        return alt_box(cx, b, &alt);
    }
    Natural {
        w: Some(Au::ZERO),
        h: Some(Au::ZERO),
        ratio: None,
    }
}

/// The natural size of a replaced element: for an image, which never loads, nothing or its alternative
/// text; an `svg` root's ratio from its `viewBox`; a canvas's from its attributes; for form controls what WebKit
/// gives them at its default control font; 300 by 150 for the rest.
pub fn natural(cx: &mut Cx, b: u32) -> Natural {
    let n = cx.tree.get(b).node;
    if cx.dom.namespace(n) == Some(Namespace::Svg) {
        let ratio = attr(cx, n, atom!("viewBox")).and_then(|v| view_box(&v));
        return match ratio {
            Some(r) => Natural {
                w: None,
                h: None,
                ratio: Some(r),
            },
            None => Natural::default(),
        };
    }
    let ty = attr(cx, n, atom!("type")).map(|t| t.to_ascii_lowercase());
    let (w, h) = match named(cx, n) {
        Some(a) if a == atom!("img") => return img(cx, b, n),
        Some(a) if a == atom!("input") => match ty.as_deref() {
            Some("checkbox" | "radio") => (12.0, 12.0),
            Some("submit" | "button" | "reset") => (60.0, 18.0),
            _ => (149.875, 15.0),
        },
        Some(a) if a == atom!("textarea") => (146.0, 26.0),
        Some(a) if a == atom!("select") => (53.0, 20.0),
        Some(a) if a == atom!("canvas") => {
            let get = |a| attr(cx, n, a).and_then(|v| dimension(&v));
            (
                get(atom!("width")).unwrap_or(300.0),
                get(atom!("height")).unwrap_or(150.0),
            )
        }
        _ => (300.0, 150.0),
    };
    Natural::sized(w, h)
}

/// The min-content and max-content widths of `b`'s content box. A percentage width or maximum, which has
/// nothing to resolve against, leaves the size its natural one, and lets it shrink to nothing.
pub fn content_widths(cx: &mut Cx, b: u32, cw: Au) -> (Au, Au) {
    let s = cx.style(b);
    let e = cx.edges(s, Au::ZERO);
    let pct = |v| matches!(v, CssSize::Lp(l) if l.has_pct());
    if !pct(s.boxm.width) && !pct(s.boxm.max_width) {
        let w = used_size(cx, b, Space::new(cw, None), &e).w;
        return (w, w);
    }
    let mut st = s.clone();
    let bm = std::sync::Arc::make_mut(&mut st.boxm);
    for v in [&mut bm.width, &mut bm.max_width] {
        if pct(*v) {
            *v = CssSize::Kw(SizeKw::Auto);
        }
    }
    let w = size_with(cx, b, &st, Space::new(cw, None), &e).w;
    (Au::ZERO, w)
}

/// The content box size of `b` in a containing block of `space` (CSS 2 10.3.2 and 10.6.2, with the
/// minimums and maximums of 10.4 applied in turn).
pub fn used_size(cx: &mut Cx, b: u32, space: Space, e: &Edges) -> Size {
    let s = cx.style(b);
    size_with(cx, b, s, space, e)
}

fn size_with(cx: &mut Cx, b: u32, s: &ComputedStyle, space: Space, e: &Edges) -> Size {
    let nat = natural(cx, b);
    let (bpi, bpb) = (e.bp_inline(), e.bp_block());
    let w = size(s.boxm.width, Some(space.width)).map(|v| content_size(s, v, bpi));
    let h = size(s.boxm.height, space.height).map(|v| content_size(s, v, bpb));
    // `aspect-ratio: auto <ratio>` takes the natural ratio first; the width and height attributes of an
    // image that does not load give it that ratio
    let ar = s.rare.aspect_ratio;
    let ratio = if ar.auto {
        nat.ratio.or(ar.ratio())
    } else {
        ar.ratio()
    };
    let by_ratio_h = |w: Au| ratio.map(|r| Au::from_px(w.px() / r));
    let by_ratio_w = |h: Au| ratio.map(|r| Au::from_px(h.px() * r));
    let avail = (space.width - e.m(1) - e.m(3) - bpi).max(Au::ZERO);
    let (w, h, auto_h, auto_w) = match (w, h) {
        (Some(w), Some(h)) => (w, h, false, false),
        (Some(w), None) => (
            w,
            by_ratio_h(w).or(nat.h).unwrap_or(Au::from_px(150.0)),
            true,
            false,
        ),
        (None, Some(h)) => (
            by_ratio_w(h).or(nat.w).unwrap_or(Au::from_px(300.0)),
            h,
            false,
            true,
        ),
        (None, None) => {
            let w = nat
                .w
                .or_else(|| nat.h.and_then(by_ratio_w))
                .unwrap_or(if ratio.is_some() {
                    avail
                } else {
                    Au::from_px(300.0)
                });
            let h = nat
                .h
                .or_else(|| by_ratio_h(w))
                .unwrap_or(Au::from_px(150.0));
            (w, h, true, true)
        }
    };
    let lim = |v, base, bp, d: Au| size(v, base).map_or(d, |x| content_size(s, x, bp));
    let b = &s.boxm;
    let (min_w, max_w) = (
        lim(b.min_width, Some(space.width), bpi, Au::ZERO),
        lim(b.max_width, Some(space.width), bpi, Au::MAX),
    );
    let (min_h, max_h) = (
        lim(b.min_height, space.height, bpb, Au::ZERO),
        lim(b.max_height, space.height, bpb, Au::MAX),
    );
    let cw = w.clamp_lo_hi(min_w, max_w);
    let mut h = if auto_h && cw != w {
        by_ratio_h(cw).unwrap_or(h)
    } else {
        h
    };
    let ch = h.clamp_lo_hi(min_h, max_h);
    let mut w = cw;
    if auto_w && ch != h {
        w = by_ratio_w(ch).unwrap_or(w).clamp_lo_hi(min_w, max_w);
    }
    h = ch;
    Size {
        w: space.fixed[0].unwrap_or(w),
        h: space.fixed[1].unwrap_or(h),
    }
}

pub fn layout(cx: &mut Cx, b: u32, space: Space) -> Laid {
    let s = cx.style(b);
    let e = cx.edges(s, space.width);
    let c = used_size(cx, b, space, &e);
    let m = margins_for(c.w, space.width, &e);
    let size = Size {
        w: c.w + e.bp_inline(),
        h: c.h + e.bp_block(),
    };
    Laid {
        frag: Fragment::new(FragKind::Box, cx.frag_node(b), size, Vec::new()),
        mt: Strut::of(e.m(0)),
        mb: Strut::of(e.m(2)),
        ml: m.ml,
        mr: m.mr,
        through: false,
        clamped: false,
        baseline: None,
        floats: false,
        statics: Vec::new(),
    }
}
