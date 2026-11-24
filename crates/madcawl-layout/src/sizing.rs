//! The used width and height of a block-level box: its size properties with their minimums, maximums and
//! `box-sizing`, auto widths filling or shrinking to fit, and auto margins taking what is left.

use madcawl_css::style::ComputedStyle;

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

fn min_max_width(s: &ComputedStyle, cw: Au, bp: Au) -> (Au, Au) {
    let min = size(s.boxm.min_width, Some(cw)).map_or(Au::ZERO, |v| content_size(s, v, bp));
    let max = size(s.boxm.max_width, Some(cw)).map_or(Au::MAX, |v| content_size(s, v, bp));
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
    let (min, max) = min_max_width(s, cw, bp);
    let w = match size(s.boxm.width, Some(cw)) {
        Some(v) => content_size(s, v, bp),
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
