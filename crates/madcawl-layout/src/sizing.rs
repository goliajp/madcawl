//! The used width and height of a block-level box: its size properties with their minimums, maximums and
//! `box-sizing`, auto widths filling or shrinking to fit, and auto margins taking what is left.

use madcawl_css::style::{ComputedStyle, Size as CssSize, SizeKw};

use crate::au::Au;
use crate::cx::{Cx, Space};
use crate::style::{Edges, content_size, size};
use crate::{intrinsic, position};

/// The used width and the left and right margins.
pub struct Inline {
    pub w: Au,
    pub ml: Au,
    pub mr: Au,
}

// a width keyword's content box width: the intrinsic ones, or what is left of the containing block
fn keyword_width(cx: &mut Cx, b: u32, v: CssSize, cw: Au, e: &Edges) -> Option<Au> {
    let CssSize::Kw(k) = v else { return None };
    let avail = (cw - e.m(1) - e.m(3) - e.bp_inline()).max(Au::ZERO);
    let widths = |cx: &mut Cx| intrinsic::content_widths(cx, b, cw);
    Some(match k {
        SizeKw::MinContent => widths(cx).0,
        SizeKw::MaxContent => widths(cx).1,
        SizeKw::FitContent => {
            let (lo, hi) = widths(cx);
            hi.min(avail).max(lo)
        }
        SizeKw::Stretch => avail,
        _ => return None,
    })
}

fn min_max_width(cx: &mut Cx, b: u32, s: &ComputedStyle, cw: Au, e: &Edges) -> (Au, Au) {
    let bp = e.bp_inline();
    let get = |cx: &mut Cx, v: CssSize| {
        size(v, Some(cw))
            .map(|v| content_size(s, v, bp))
            .or_else(|| keyword_width(cx, b, v, cw, e))
    };
    let min = get(cx, s.boxm.min_width).unwrap_or(Au::ZERO);
    let max = get(cx, s.boxm.max_width).unwrap_or(Au::MAX);
    (min, max)
}

// CSS 2 10.3.3: width and margins of a block-level box in normal flow, auto margins taking what is left
pub fn margins_for(w: Au, cw: Au, e: &Edges) -> Inline {
    let left = cw - w - e.bp_inline() - e.m(1) - e.m(3);
    let (ml, mr) = match (e.margin[3], e.margin[1]) {
        (None, None) if left > Au::ZERO => (left.half(), left - left.half()),
        (None, None) => (Au::ZERO, Au::ZERO),
        (None, Some(mr)) => (left.max(Au::ZERO), mr),
        (Some(ml), _) => (ml, cw - w - e.bp_inline() - ml),
    };
    Inline { w, ml, mr }
}

pub fn used_width(
    cx: &mut Cx,
    b: u32,
    s: &ComputedStyle,
    space: Space,
    e: &Edges,
    shrink: bool,
) -> Inline {
    let cw = space.width;
    if let Some(w) = space.fixed[0] {
        return margins_for(w, cw, e);
    }
    let bp = e.bp_inline();
    let (min, max) = min_max_width(cx, b, s, cw, e);
    let w = match size(s.boxm.width, Some(cw)) {
        Some(v) => content_size(s, v, bp),
        None if let Some(v) = keyword_width(cx, b, s.boxm.width, cw, e) => v,
        None if let Some(v) = position::stretched_width(s, cw, e) => v,
        None if shrink => {
            let (lo, hi) = intrinsic::content_widths(cx, b, cw);
            let avail = (cw - e.m(1) - e.m(3) - bp).max(Au::ZERO);
            hi.min(avail).max(lo)
        }
        None => {
            let w = (cw - e.m(1) - e.m(3) - bp).max(Au::ZERO);
            if w <= max && w >= min {
                return Inline {
                    w,
                    ml: e.m(3),
                    mr: e.m(1),
                };
            }
            w
        }
    };
    margins_for(w.clamp_lo_hi(min, max), cw, e)
}

pub fn used_height(s: &ComputedStyle, cb_h: Option<Au>, bp: Au) -> (Option<Au>, Au, Au) {
    let h = size(s.boxm.height, cb_h).map(|v| content_size(s, v, bp));
    let min = size(s.boxm.min_height, cb_h).map_or(Au::ZERO, |v| content_size(s, v, bp));
    let max = size(s.boxm.max_height, cb_h).map_or(Au::MAX, |v| content_size(s, v, bp));
    (h, min, max)
}

// the body's overflow given to the viewport starts no formatting context of its own
pub fn body_overflow(cx: &Cx, b: u32, s: &madcawl_css::style::ComputedStyle) -> bool {
    cx.tree.get(b).parent != crate::tree::NONE
        && cx.overflow_propagated(b)
        && matches!(
            s.display,
            madcawl_css::style::Display::Block | madcawl_css::style::Display::ListItem
        )
        && s.float == madcawl_css::style::Float::None
        && !position::out_of_flow(s)
}

// an auto height from `aspect-ratio` and the used width, in the box `box-sizing` names
pub fn ratio_height(s: &madcawl_css::style::ComputedStyle, w: Au, e: &Edges) -> Option<Au> {
    let r = s.rare.aspect_ratio.ratio()?;
    Some(match s.boxm.box_sizing {
        madcawl_css::style::BoxSizing::BorderBox => {
            (Au::from_px((w + e.bp_inline()).px() / r) - e.bp_block()).max(Au::ZERO)
        }
        madcawl_css::style::BoxSizing::ContentBox => Au::from_px(w.px() / r),
    })
}
