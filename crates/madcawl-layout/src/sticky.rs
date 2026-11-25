//! `position: sticky` with nothing scrolled: once the document is laid out, each sticky box is moved, as a
//! relatively positioned one is, by what keeps it inside its nearest scroll container's scrollport inset by
//! its `top`, `right`, `bottom` and `left`, no further than its containing block's content box lets it go.
//! The fragments from the root down to each moved box are made again; the rest are shared. A fragment is
//! found sticky by its element's computed values, so a `::before` or `::after` box, whose fragments have no
//! element, is not moved.

use std::sync::Arc;

use madcawl_css::style::{ComputedStyle, Position, Styles};

use crate::au::{Au, Point, Rect};
use crate::cx::device_floor;
use crate::fragment::{Child, FragKind, Fragment};
use crate::style::{Edges, size};

/// The root's fragment placed at `root.offset` with its sticky boxes moved, the viewport being the
/// document's scrollport.
pub fn apply(styles: &Styles, scale: f32, root: Child, viewport: Rect) -> Child {
    let frag = visit((styles, scale), &root.frag, root.offset, viewport, viewport).unwrap_or(root.frag);
    Child {
        offset: root.offset,
        frag,
    }
}

// the edges layout gave the box: its borders whole device pixels at `scale`
fn edges(s: &ComputedStyle, cw: Au, scale: f32) -> Edges {
    let mut e = Edges::of(s, cw);
    for b in &mut e.border {
        *b = device_floor(*b, scale);
    }
    e
}

fn content_box(s: &ComputedStyle, scale: f32, at: Point, f: &Fragment) -> Rect {
    let e = edges(s, f.size.w, scale);
    Rect {
        x: at.x + e.bp(3),
        y: at.y + e.bp(0),
        w: f.size.w - e.bp_inline(),
        h: f.size.h - e.bp_block(),
    }
}

fn padding_box(s: &ComputedStyle, scale: f32, at: Point, f: &Fragment) -> Rect {
    let b = edges(s, f.size.w, scale).border;
    Rect {
        x: at.x + b[3],
        y: at.y + b[0],
        w: f.size.w - b[1] - b[3],
        h: f.size.h - b[0] - b[2],
    }
}

// how far one axis moves: `start` and `end` the insets, `pos` and `len` the box, `port` and `cb` the
// scrollport and the containing block, each as (start, length); the start inset wins over the end one
fn shift(
    start: Option<Au>,
    end: Option<Au>,
    (pos, len): (Au, Au),
    port: (Au, Au),
    cb: (Au, Au),
) -> Au {
    let mut d = Au::ZERO;
    if let Some(e) = end {
        let limit = port.0 + port.1 - e;
        if pos + len > limit {
            d = limit - (pos + len);
        }
    }
    if let Some(s) = start {
        let limit = port.0 + s;
        if pos + d < limit {
            d = limit - pos;
        }
    }
    if d > Au::ZERO {
        d.min((cb.0 + cb.1 - (pos + len)).max(Au::ZERO))
    } else {
        d.max((cb.0 - pos).min(Au::ZERO))
    }
}

fn sticky_offset(s: &ComputedStyle, r: Rect, port: Rect, cb: Rect) -> Point {
    let i = &s.place.inset;
    let (pw, ph) = (Some(port.w), Some(port.h));
    // its margin box stays in the containing block
    let e = Edges::of(s, cb.w);
    let cb = Rect {
        x: cb.x + e.m(3),
        y: cb.y + e.m(0),
        w: cb.w - e.m(1) - e.m(3),
        h: cb.h - e.m(0) - e.m(2),
    };
    Point {
        x: shift(
            size(i[3], pw),
            size(i[1], pw),
            (r.x, r.w),
            (port.x, port.w),
            (cb.x, cb.w),
        ),
        y: shift(
            size(i[0], ph),
            size(i[2], ph),
            (r.y, r.h),
            (port.y, port.h),
            (cb.y, cb.h),
        ),
    }
}

// the fragment at `at` made again when a sticky box at or below it moves; recursion is bounded by the
// fragment tree's depth, which the box tree's bounds
fn visit(
    (styles, scale): (&Styles, f32),
    f: &Arc<Fragment>,
    at: Point,
    port: Rect,
    cb: Rect,
) -> Option<Arc<Fragment>> {
    let style = (f.kind == FragKind::Box)
        .then(|| f.node.and_then(|n| styles.get(n)))
        .flatten();
    let (inner_cb, inner_port) = match style {
        Some(s) => (
            content_box(s, scale, at, f),
            if f.scroller {
                padding_box(s, scale, at, f)
            } else {
                port
            },
        ),
        None => (cb, port),
    };
    let mut kids: Option<Vec<Child>> = None;
    for (k, c) in f.children.iter().enumerate() {
        let cat = at + c.offset;
        let below = visit((styles, scale), &c.frag, cat, inner_port, inner_cb);
        let cs = (c.frag.kind == FragKind::Box)
            .then(|| c.frag.node.and_then(|n| styles.get(n)))
            .flatten()
            .filter(|s| s.position == Position::Sticky);
        let moved = cs.map_or(Point::default(), |s| {
            let r = Rect {
                x: cat.x,
                y: cat.y,
                w: c.frag.size.w,
                h: c.frag.size.h,
            };
            sticky_offset(s, r, inner_port, inner_cb)
        });
        if below.is_none() && moved == Point::default() {
            continue;
        }
        let v = kids.get_or_insert_with(|| f.children.to_vec());
        v[k] = Child {
            offset: c.offset + moved,
            frag: below.unwrap_or_else(|| c.frag.clone()),
        };
    }
    let kids = kids?;
    Some(Fragment::with_scroller(
        f.kind, f.node, f.size, kids, f.scroller,
    ))
}
