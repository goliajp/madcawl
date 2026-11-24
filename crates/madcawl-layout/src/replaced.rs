//! Replaced elements: their natural sizes, which this crate does not load, are the defaults HTML gives
//! them, and they are sized from those and their computed values.

use madcawl_dom::NodeId;
use madcawl_html::Namespace;
use madcawl_str::atom;

use crate::au::{Au, Size};
use crate::cx::{Cx, Laid, Space, Strut};
use crate::fragment::{FragKind, Fragment};
use crate::sizing::margins_for;
use crate::style::{Edges, content_size, size};

fn named(cx: &Cx, n: NodeId) -> Option<madcawl_str::Atom> {
    (cx.dom.namespace(n) == Some(Namespace::Html))
        .then(|| cx.dom.local_name(n))
        .flatten()
}

/// The content size a replaced element has when its computed values do not give one: 300 by 150 for what
/// draws its own content, nothing for an image whose data is not loaded, and for form controls what WebKit
/// gives them at its default control font.
pub fn natural_size(cx: &Cx, b: u32) -> Size {
    let n = cx.tree.get(b).node;
    let ty = cx
        .dom
        .attribute(n, atom!("type"))
        .and_then(|v| v.as_ascii_str())
        .map(|t| t.to_ascii_lowercase());
    let (w, h) = match named(cx, n) {
        Some(a) if a == atom!("img") => (0.0, 0.0),
        Some(a) if a == atom!("input") => match ty.as_deref() {
            Some("checkbox" | "radio") => (12.0, 12.0),
            Some("submit" | "button" | "reset") => (60.0, 18.0),
            _ => (149.875, 15.0),
        },
        Some(a) if a == atom!("textarea") => (146.0, 26.0),
        Some(a) if a == atom!("select") => (53.0, 20.0),
        _ => (300.0, 150.0),
    };
    Size {
        w: Au::from_px(w),
        h: Au::from_px(h),
    }
}

pub fn layout(cx: &mut Cx, b: u32, space: Space) -> Laid {
    let s = cx.style(b);
    let e = Edges::of(s, space.width);
    let nat = natural_size(cx, b);
    let (bpi, bpb) = (e.bp_inline(), e.bp_block());
    let w = size(s.boxm.width, Some(space.width)).map(|v| content_size(s, v, bpi));
    let h = size(s.boxm.height, space.height).map(|v| content_size(s, v, bpb));
    let ratio = (nat.h != Au::ZERO).then(|| nat.w.px() / nat.h.px());
    let (w, h) = match (w, h) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, ratio.map_or(nat.h, |r| Au::from_px(w.px() / r))),
        (None, Some(h)) => (ratio.map_or(nat.w, |r| Au::from_px(h.px() * r)), h),
        (None, None) => (nat.w, nat.h),
    };
    let min_w =
        size(s.boxm.min_width, Some(space.width)).map_or(Au::ZERO, |v| content_size(s, v, bpi));
    let max_w =
        size(s.boxm.max_width, Some(space.width)).map_or(Au::MAX, |v| content_size(s, v, bpi));
    let min_h = size(s.boxm.min_height, space.height).map_or(Au::ZERO, |v| content_size(s, v, bpb));
    let max_h = size(s.boxm.max_height, space.height).map_or(Au::MAX, |v| content_size(s, v, bpb));
    let w = space.fixed[0].unwrap_or(w.clamp_lo_hi(min_w, max_w));
    let h = space.fixed[1].unwrap_or(h.clamp_lo_hi(min_h, max_h));
    let m = margins_for(w, space.width, &e);
    let size = Size {
        w: w + bpi,
        h: h + bpb,
    };
    Laid {
        frag: Fragment::new(FragKind::Box, Some(cx.tree.get(b).node), size, Vec::new()),
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
