//! Grid items: what they contribute to the tracks, where the tracks are once aligned (`justify-content`,
//! `align-content`), and each item laid out in its grid area and aligned there (`justify-self`,
//! `align-self`, auto margins).

use madcawl_css::style::{Align, ComputedStyle, Overflow, Size as CssSize, SizeKw};

use crate::au::{Au, Point};
use crate::block;
use crate::cx::{Cx, Laid, Space};
use crate::fragment::Child;
use crate::style::{Edges, content_size, size};
use crate::tree::Kind;
use crate::{intrinsic, position};

use super::tracks::{Contrib, MaxFn, Track};
use super::{GItem, Grid, item_style};

pub fn content_stretches(a: Align) -> bool {
    matches!(a, Align::Normal | Align::Stretch)
}

/// The tracks `first..end` and the gaps between them.
pub fn span_size(ts: &[Track], gap: Au, first: usize, end: usize) -> Au {
    let r = &ts[first..end];
    let n = r.iter().filter(|t| !t.collapsed).count();
    r.iter().fold(
        Au(gap.0.saturating_mul(n.saturating_sub(1) as i32)),
        |a, t| a + t.base,
    )
}

/// Where each track starts, and after them where the last ends, with the free space of `avail`
/// distributed by `align`.
pub fn positions(ts: &[Track], gap: Au, avail: Au, align: Align) -> Vec<Au> {
    let n = ts.iter().filter(|t| !t.collapsed).count() as i32;
    let free = (avail - span_size(ts, gap, 0, ts.len())).max(Au::ZERO);
    let (start, extra) = match align {
        Align::End | Align::FlexEnd | Align::Right => (free, Au::ZERO),
        Align::Center => (free.half(), Au::ZERO),
        Align::SpaceBetween if n > 1 => (Au::ZERO, Au(free.0 / (n - 1))),
        Align::SpaceAround if n > 0 => (Au(free.0 / (2 * n)), Au(free.0 / n)),
        Align::SpaceEvenly if n > 0 => (Au(free.0 / (n + 1)), Au(free.0 / (n + 1))),
        _ => (Au::ZERO, Au::ZERO),
    };
    let mut out = Vec::with_capacity(ts.len() + 1);
    let mut at = start;
    let mut seen = false;
    for t in ts {
        if !t.collapsed && seen {
            at += gap + extra;
        }
        out.push(at);
        at += t.base;
        seen |= !t.collapsed;
    }
    out.push(at);
    out
}

fn area(pos: &[Au], ts: &[Track], (first, end): (usize, usize)) -> (Au, Au) {
    let last = end - 1;
    (pos[first], pos[last] + ts[last].base - pos[first])
}

fn is_auto(s: CssSize) -> bool {
    matches!(s, CssSize::Kw(SizeKw::Auto))
}

fn crosses_flex(ts: &[Track], (first, end): (usize, usize)) -> bool {
    end - first > 1 && ts[first..end].iter().any(|t| matches!(t.max, MaxFn::Fr(_)))
}

// the sum of the fixed maximums of the tracks `first..end` and the gaps between them, when every one has
// a fixed maximum: an automatic minimum goes no further (css grid 6.6; webkit does not limit it by a
// `fit-content()` argument)
fn fixed_max(ts: &[Track], (first, end): (usize, usize), gap: Au) -> Option<Au> {
    let mut sum = Au(gap.0.saturating_mul((end - first) as i32 - 1));
    for t in &ts[first..end] {
        sum += match t.max {
            MaxFn::Fixed(v) => v,
            _ => return None,
        };
    }
    Some(sum)
}

/// Each item's contribution to the columns it spans: its min-content and max-content widths, and its
/// minimum, which is its min-content width when `min-width` is `auto` and it does not scroll.
pub fn column_contribs(cx: &mut Cx, g: &Grid, cw: Au) -> Vec<Contrib> {
    g.items
        .iter()
        .map(|it| {
            let (lo, hi) = intrinsic::margin_box_widths(cx, it.b, cw);
            let s = item_style(cx, it.b);
            let e = edges(cx, it.b, s, Au::ZERO);
            let outer = e.m(1) + e.m(3) + e.bp_inline();
            let minimum = match size(s.boxm.min_width, None) {
                Some(v) => content_size(s, v, e.bp_inline()) + outer,
                None if s.place.overflow_x == Overflow::Visible
                    && !crosses_flex(&g.cols, it.cols) =>
                {
                    fixed_max(&g.cols, it.cols, g.col_gap).map_or(lo, |m| lo.min(m.max(outer)))
                }
                None => outer,
            };
            Contrib {
                first: it.cols.0,
                end: it.cols.1,
                min_content: lo,
                max_content: hi,
                minimum,
            }
        })
        .collect()
}

fn edges(cx: &Cx, b: u32, s: &ComputedStyle, cw: Au) -> Edges {
    if cx.has_edges(b) {
        Edges::of(s, cw)
    } else {
        Edges::default()
    }
}

fn justify(s: &ComputedStyle, parent: &ComputedStyle) -> Align {
    match s.rare.justify_self {
        Align::Auto => parent.rare.justify_items,
        a => a,
    }
}

fn align(s: &ComputedStyle, parent: &ComputedStyle) -> Align {
    match s.rare.align_self {
        Align::Auto => parent.rare.align_items,
        a => a,
    }
}

// whether an item fills its area along an axis: a stretching or `normal` alignment, an auto size, no
// auto margin; a replaced element is not stretched by `normal`
fn fills(a: Align, auto_size: bool, auto_margin: bool, replaced: bool) -> bool {
    auto_size && !auto_margin && (a == Align::Stretch || a == Align::Normal && !replaced)
}

/// The space an item is laid out in for an area `w` wide: the width it takes fixed when its own is auto.
fn space_for(cx: &mut Cx, g: &GItem, parent: &ComputedStyle, w: Au, h: Option<Au>) -> Space {
    let s = item_style(cx, g.b);
    let e = edges(cx, g.b, s, w);
    let replaced = matches!(cx.tree.get(g.b).kind, Kind::Replaced { .. });
    let mut sp = Space::new(w, h);
    let kw = match s.boxm.width {
        CssSize::Kw(k) => k,
        CssSize::Lp(_) => return sp,
    };
    if replaced {
        return sp;
    }
    let room = (w - e.m(1) - e.m(3) - e.bp_inline()).max(Au::ZERO);
    let auto_m = e.margin[1].is_none() || e.margin[3].is_none();
    let cw = if kw == SizeKw::Auto && fills(justify(s, parent), true, auto_m, false) {
        room
    } else {
        let (lo, hi) = intrinsic::content_widths(cx, g.b, w);
        match kw {
            SizeKw::MinContent => lo,
            SizeKw::MaxContent => hi,
            _ => hi.min(room).max(lo),
        }
    };
    let min =
        size(s.boxm.min_width, Some(w)).map_or(Au::ZERO, |v| content_size(s, v, e.bp_inline()));
    let max =
        size(s.boxm.max_width, Some(w)).map_or(Au::MAX, |v| content_size(s, v, e.bp_inline()));
    sp.fixed[0] = Some(cw.clamp_lo_hi(min, max));
    sp
}

/// Each item's height at the width of its columns, as its contribution to its rows.
pub fn row_contribs(cx: &mut Cx, b: u32, g: &Grid, cols: &[Au]) -> Vec<Contrib> {
    let parent = cx.style(b);
    g.items
        .iter()
        .map(|it| {
            let (_, w) = area(cols, &g.cols, it.cols);
            let sp = space_for(cx, it, parent, w, None);
            let r = block::layout(cx, it.b, sp);
            let e = edges(cx, it.b, item_style(cx, it.b), w);
            let h = r.frag.size.h + e.m(0) + e.m(2);
            Contrib {
                first: it.rows.0,
                end: it.rows.1,
                min_content: h,
                max_content: h,
                minimum: h,
            }
        })
        .collect()
}

// where in its area of `room` an item of outer size `outer` starts
fn offset(a: Align, room: Au, outer: Au, auto: (bool, bool)) -> Au {
    let free = room - outer;
    match auto {
        (true, true) => return free.max(Au::ZERO).half(),
        (true, false) => return free.max(Au::ZERO),
        (false, true) => return Au::ZERO,
        _ => {}
    }
    match a {
        Align::End | Align::FlexEnd | Align::SelfEnd | Align::Right => free,
        Align::Center => free.half(),
        _ => Au::ZERO,
    }
}

type Placed = (Vec<Child>, Vec<(u32, Point)>, Option<Au>);

/// Lays each item out in its grid area and aligns it there.
pub fn place(cx: &mut Cx, b: u32, g: &Grid, cols: &[Au], rows: &[Au]) -> Placed {
    let parent = cx.style(b);
    let (mut out, mut statics) = (Vec::with_capacity(g.items.len()), Vec::new());
    let mut first: Option<((usize, usize), Au)> = None;
    for it in &g.items {
        let s = item_style(cx, it.b);
        let (x, w) = area(cols, &g.cols, it.cols);
        let (y, h) = area(rows, &g.rows, it.rows);
        let e = edges(cx, it.b, s, w);
        let replaced = matches!(cx.tree.get(it.b).kind, Kind::Replaced { .. });
        let mut sp = space_for(cx, it, parent, w, Some(h));
        let auto_v = (e.margin[0].is_none(), e.margin[2].is_none());
        let va = align(s, parent);
        if fills(va, is_auto(s.boxm.height), auto_v.0 || auto_v.1, replaced) {
            let bp = e.bp_block();
            let min = size(s.boxm.min_height, Some(h)).map_or(Au::ZERO, |v| content_size(s, v, bp));
            let max = size(s.boxm.max_height, Some(h)).map_or(Au::MAX, |v| content_size(s, v, bp));
            let fill = (h - e.m(0) - e.m(2) - bp).max(Au::ZERO);
            sp.fixed[1] = Some(fill.clamp_lo_hi(min, max));
        }
        let r: Laid = block::layout(cx, it.b, sp);
        let sz = r.frag.size;
        let auto_h = (e.margin[3].is_none(), e.margin[1].is_none());
        let dx = offset(justify(s, parent), w, sz.w + e.m(1) + e.m(3), auto_h);
        let dy = offset(va, h, sz.h + e.m(0) + e.m(2), auto_v);
        let at = Point {
            x: x + dx + e.m(3),
            y: y + dy + e.m(0),
        };
        let pos = at + position::relative_offset(s, Space::new(w, Some(h)), cx.has_edges(it.b));
        if let Some(bl) = r.baseline
            && first.is_none_or(|(k, _)| (it.rows.0, it.cols.0) < k)
        {
            first = Some(((it.rows.0, it.cols.0), pos.y + bl));
        }
        statics.extend(r.statics.iter().map(|&(a, p)| (a, p + pos)));
        out.push(Child {
            offset: pos,
            frag: r.frag,
        });
    }
    (out, statics, first.map(|(_, b)| b))
}
