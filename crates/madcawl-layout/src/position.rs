//! Positioning: relative offsets, and absolutely and fixed positioned boxes laid out against their
//! containing block once it has its size.

use madcawl_css::style::{ComputedStyle, Float, Position};

use crate::au::{Au, Point, Rect, Size};
use crate::block;
use crate::cx::{Cx, Space};
use crate::fragment::Child;
use crate::style::{Edges, size};
use crate::tree::NONE;

pub fn out_of_flow(s: &ComputedStyle) -> bool {
    matches!(s.position, Position::Absolute | Position::Fixed)
}

/// Whether an `auto` width shrinks to fit the contents.
pub fn shrinks(s: &ComputedStyle) -> bool {
    s.float != Float::None || out_of_flow(s) && !stretches(s)
}

// both `left` and `right` are set: the width is what they leave
fn stretches(s: &ComputedStyle) -> bool {
    let i = &s.place.inset;
    out_of_flow(s) && !is_auto(i[1]) && !is_auto(i[3])
}

fn is_auto(v: madcawl_css::style::Size) -> bool {
    matches!(v, madcawl_css::style::Size::Kw(_))
}

/// The `left` and `top` of a relatively positioned box, or the negated `right` and `bottom`.
pub fn relative_offset(s: &ComputedStyle, cb: Space, edges: bool) -> Point {
    if !edges || s.position != Position::Relative {
        return Point::default();
    }
    let i = &s.place.inset;
    let x = size(i[3], Some(cb.width))
        .or_else(|| size(i[1], Some(cb.width)).map(|v| -v))
        .unwrap_or(Au::ZERO);
    let y = size(i[0], cb.height)
        .or_else(|| size(i[2], cb.height).map(|v| -v))
        .unwrap_or(Au::ZERO);
    Point { x, y }
}

/// The width an absolutely positioned box with both `left` and `right` gets, if it has them.
pub fn stretched_width(s: &ComputedStyle, cw: Au, e: &Edges) -> Option<Au> {
    if !stretches(s) || !is_auto(s.boxm.width) {
        return None;
    }
    let l = size(s.place.inset[3], Some(cw))?;
    let r = size(s.place.inset[1], Some(cw))?;
    Some((cw - l - r - e.m(1) - e.m(3) - e.bp_inline()).max(Au::ZERO))
}

/// Lays out the out-of-flow boxes in `statics` (with their static positions) whose containing block is `b`
/// (border box `size`, borders in `e`) and adds them to its children; the others are for a containing
/// block above it.
pub fn place_absolute(
    cx: &mut Cx,
    b: u32,
    size_b: Size,
    e: &Edges,
    kids: &mut Vec<Child>,
    statics: Vec<(u32, Point)>,
) -> Vec<(u32, Point)> {
    let root = cx.tree.get(b).parent == NONE;
    if statics.is_empty() || !root && (!cx.has_edges(b) || cx.style(b).position == Position::Static)
    {
        return statics;
    }
    let cb = if root {
        Rect {
            x: Au::ZERO,
            y: Au::ZERO,
            w: cx.icb.w,
            h: cx.icb.h,
        }
    } else {
        Rect {
            x: e.border[3],
            y: e.border[0],
            w: size_b.w - e.border[1] - e.border[3],
            h: size_b.h - e.border[0] - e.border[2],
        }
    };
    place_in(cx, cb, root, kids, statics)
}

/// Lays out the out-of-flow boxes in `statics` against the containing block `cb`, in the coordinates of
/// `kids`, which they are added to; the fixed ones, unless `root`, are left for the viewport.
pub fn place_in(
    cx: &mut Cx,
    cb: Rect,
    root: bool,
    kids: &mut Vec<Child>,
    statics: Vec<(u32, Point)>,
) -> Vec<(u32, Point)> {
    let (cbx, cby, cw, ch) = (cb.x, cb.y, cb.w, cb.h);
    let (mut pending, mut up, mut k) = (statics, Vec::new(), 0);
    while let Some(&(a, st)) = pending.get(k) {
        k += 1;
        let s = cx.style(a);
        // a fixed box's containing block is the viewport
        if !root && s.position == Position::Fixed {
            up.push((a, st));
            continue;
        }
        let i = &s.place.inset;
        let me = cx.edges(s, cw);
        let mut space = Space::new(cw, Some(ch));
        // `top` and `bottom` both set and no height: the height is what they leave
        if is_auto(s.boxm.height)
            && let (Some(t), Some(bt)) = (size(i[0], Some(ch)), size(i[2], Some(ch)))
        {
            space.fixed[1] = Some((ch - t - bt - me.m(0) - me.m(2) - me.bp_block()).max(Au::ZERO));
        }
        let r = block::layout(cx, a, space);
        let sz = r.frag.size;
        let x = match (size(i[3], Some(cw)), size(i[1], Some(cw))) {
            (Some(l), _) => cbx + l + me.m(3),
            (None, Some(rt)) => cbx + cw - rt - me.m(1) - sz.w,
            _ => st.x + me.m(3),
        };
        let y = match (size(i[0], Some(ch)), size(i[2], Some(ch))) {
            (Some(t), _) => cby + t + me.m(0),
            (None, Some(bt)) => cby + ch - bt - me.m(2) - sz.h,
            _ => st.y + me.m(0),
        };
        let at = Point { x, y };
        // the fixed boxes below it, which the root places too
        let below = r.statics.iter().map(|&(f, p)| (f, p + at));
        if root {
            pending.extend(below);
        } else {
            up.extend(below);
        }
        kids.push(Child {
            offset: at,
            frag: r.frag,
        });
    }
    up
}
