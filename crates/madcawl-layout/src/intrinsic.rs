//! Intrinsic widths: the min-content and max-content widths that shrink-to-fit sizing takes, computed once
//! per box and layout.

use crate::au::Au;
use crate::cx::Cx;
use crate::style::{Edges, content_size, size};
use crate::tree::Kind;
use crate::{inline, position, replaced};

/// The min-content and max-content widths of the content box of block container `b`.
pub fn content_widths(cx: &mut Cx, b: u32, cw: Au) -> (Au, Au) {
    if let Some(v) = cx.intrinsic[b as usize] {
        return v;
    }
    let v = if cx.has_edges(b) && crate::flex::is_flex(cx.style(b)) {
        crate::flex::content_widths(cx, b, cw)
    } else if cx.tree.has_inline_children(b) {
        inline::content_widths(cx, b, cw)
    } else {
        let kids: Vec<u32> = cx.tree.children(b).collect();
        let (mut lo, mut hi) = (Au::ZERO, Au::ZERO);
        for c in kids {
            if cx.has_edges(c) && position::out_of_flow(cx.style(c)) {
                continue;
            }
            let (a, z) = margin_box_widths(cx, c, cw);
            lo = lo.max(a);
            hi = hi.max(z);
        }
        (lo, hi)
    };
    cx.intrinsic[b as usize] = Some(v);
    v
}

/// The same of the margin box of a block-level or atomic box; percentages of the unknown width count as 0.
pub fn margin_box_widths(cx: &mut Cx, b: u32, cw: Au) -> (Au, Au) {
    if !cx.has_edges(b) {
        return content_widths(cx, b, cw);
    }
    let s = cx.style(b);
    let e = Edges::of(s, Au::ZERO);
    let bp = e.bp_inline();
    let outer = e.m(1) + e.m(3) + bp;
    let fixed = size(s.boxm.width, None).map(|v| content_size(s, v, bp));
    let (lo, hi) = match (fixed, cx.tree.get(b).kind) {
        (Some(w), _) => (w, w),
        (None, Kind::Replaced { .. }) => {
            let w = replaced::natural_size(cx, b).w;
            (w, w)
        }
        (None, _) => content_widths(cx, b, cw),
    };
    let min = size(s.boxm.min_width, None).map_or(Au::ZERO, |v| content_size(s, v, bp));
    let max = size(s.boxm.max_width, None).map_or(Au::MAX, |v| content_size(s, v, bp));
    (
        lo.clamp_lo_hi(min, max) + outer,
        hi.clamp_lo_hi(min, max) + outer,
    )
}
