//! Intrinsic widths: the min-content and max-content widths that shrink-to-fit sizing takes, computed once
//! per box and layout.

use crate::au::Au;
use crate::cx::Cx;
use crate::style::{content_size, size};
use crate::tree::Kind;
use madcawl_css::style::Size as CssSize;
use crate::{inline, position, replaced};

/// The min-content and max-content widths of the content box of block container `b`.
pub fn content_widths(cx: &mut Cx, b: u32, cw: Au) -> (Au, Au) {
    if let Some(v) = cx.intrinsic[b as usize] {
        return v;
    }
    let v = if let Kind::Replaced { .. } = cx.tree.get(b).kind {
        replaced::content_widths(cx, b, cw)
    } else if cx.has_edges(b) && crate::flex::is_flex(cx.style(b)) {
        crate::flex::content_widths(cx, b, cw)
    } else if cx.has_edges(b) && crate::grid::is_grid(cx.style(b)) {
        crate::grid::content_widths(cx, b, cw)
    } else if crate::table::is_table(cx, b) {
        crate::table::content_widths(cx, b, cw)
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
    let e = cx.edges(s, Au::ZERO);
    let bp = e.bp_inline();
    let outer = e.m(1) + e.m(3) + bp;
    let fixed = size(s.boxm.width, None).map(|v| content_size(s, v, bp));
    let replaced = matches!(cx.tree.get(b).kind, Kind::Replaced { .. });
    let (lo, hi) = match fixed {
        Some(w) => (w, w),
        None if replaced => replaced::content_widths(cx, b, cw),
        None => content_widths(cx, b, cw),
    };
    let lo = if replaced && compressible(s) { Au::ZERO } else { lo };
    let min = size(s.boxm.min_width, None).map_or(Au::ZERO, |v| content_size(s, v, bp));
    let max = size(s.boxm.max_width, None).map_or(Au::MAX, |v| content_size(s, v, bp));
    (
        lo.clamp_lo_hi(min, max) + outer,
        hi.clamp_lo_hi(min, max) + outer,
    )
}

// a replaced box sized by a percentage of its container's width can shrink to nothing at min-content
// (css sizing 3 5.2.2); webkit does the same for a percentage max-width
fn compressible(s: &madcawl_css::style::ComputedStyle) -> bool {
    let pct = |v: CssSize| matches!(v, CssSize::Lp(l) if l.has_pct());
    pct(s.boxm.width) || pct(s.boxm.max_width)
}
