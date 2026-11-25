//! Block layout: widths and heights with their minimums, maximums and `box-sizing`, margins with collapsing,
//! and block-level children placed one under the other.

use crate::au::{Au, Point, Size};
use crate::cx::{Cx, Laid, Space, Strut};
use crate::float::Exclusions;
use crate::fragment::{Child, FragKind, Fragment};
use crate::sizing::{Inline, used_height, used_width};
use crate::style::{Edges, new_formatting_context};
use crate::tree::Kind;
use crate::{cache, flex, grid, inline, position, replaced, sizing, table};

/// The block-level box `b` in `space`, or the last result when nothing changed; `at` is where its border box
/// is in its block formatting context, which floats are placed in.
pub fn layout_at(cx: &mut Cx, b: u32, space: Space, at: Point) -> Laid {
    let no_floats = cx.excl.last().is_none_or(|e| e.is_empty());
    if cx.reuse
        && no_floats
        && let Some(laid) = cache::lookup(cx, b, &space)
        && !laid.floats
    {
        cx.counters.reused += 1;
        return laid;
    }
    cx.counters.laid += 1;
    let floats_before = cx.excl.last().map_or(0, |e| e.len());
    let kind = cx.tree.get(b).kind;
    let mut laid = match kind {
        Kind::Replaced { .. } => replaced::layout(cx, b, space),
        _ if table::is_table(cx, b) => table::layout(cx, b, space),
        _ => block(cx, b, space, at),
    };
    // floats it placed in the formatting context around it are placed again only by laying it out again
    laid.floats |= !no_floats || cx.excl.last().map_or(0, |e| e.len()) > floats_before;
    // an atomic inline's auto margins are zero
    if kind.is_inline_level() || cx.style(b).float != madcawl_css::style::Float::None {
        let e = cx.edges(cx.style(b), space.width);
        (laid.ml, laid.mr) = (e.m(3), e.m(1));
    }
    cache::store(cx, b, space, &laid);
    laid
}

/// The same for a box that starts its own formatting context, where its position does not matter.
pub fn layout(cx: &mut Cx, b: u32, space: Space) -> Laid {
    layout_at(cx, b, space, Point::default())
}

struct Flow {
    children: Vec<Child>,
    cursor: Au,
    strut: Strut,
    /// Nothing placed yet: margins still adjoin the box's top
    first: bool,
    baseline: Option<Au>,
    /// Where the content box is in the formatting context
    at: Point,
    /// Out-of-flow boxes passed, from the border box corner
    statics: Vec<(u32, Point)>,
}

fn block(cx: &mut Cx, b: u32, space: Space, at: Point) -> Laid {
    let s = cx.style(b);
    let edges = cx.has_edges(b);
    let e = if edges {
        cx.edges(s, space.width)
    } else {
        Edges::default()
    };
    let root = cx.tree.get(b).parent == crate::tree::NONE;
    let fc = edges && (new_formatting_context(s, root) && !sizing::body_overflow(cx, b, s))
        || flex::is_item(cx, b)
        || grid::is_item(cx, b);
    let shrink = cx.tree.get(b).kind == Kind::InlineBlock || position::shrinks(s);
    let iw = if edges {
        used_width(cx, b, s, space, &e, shrink)
    } else {
        Inline {
            w: space.fixed[0].unwrap_or(space.width),
            ml: Au::ZERO,
            mr: Au::ZERO,
        }
    };
    let (h, min_h, max_h) = if let Some(h) = space.fixed[1] {
        (Some(h), h, h)
    } else if edges {
        let (h, lo, hi) = used_height(s, space.height, e.bp_block());
        (h.or_else(|| sizing::ratio_height(s, iw.w, &e)), lo, hi)
    } else {
        (None, Au::ZERO, Au::MAX)
    };
    let inner = Space::new(iw.w, h.map(|h| h.clamp_lo_hi(min_h, max_h)));
    let mut mt = Strut::of(e.m(0));
    let origin = Point {
        x: e.bp(3),
        y: e.bp(0),
    };
    let base = if fc { Point::default() } else { at };
    let mut flow = Flow {
        children: Vec::new(),
        cursor: Au::ZERO,
        strut: Strut::default(),
        first: true,
        baseline: None,
        at: base + origin,
        statics: Vec::new(),
    };
    if fc {
        cx.excl.push(Exclusions::default());
    }
    let absorb_top = !fc && e.bp(0) == Au::ZERO;
    if edges && flex::is_flex(s) {
        let r = flex::layout(cx, b, inner, (min_h, max_h));
        flow.first = r.children.is_empty();
        flow.cursor = r.height;
        flow.baseline = r.baseline.map(|v| v + origin.y);
        flow.children = r.children;
        flow.statics = r.statics;
        from_content(&mut flow, origin);
    } else if edges && grid::is_grid(s) {
        let r = grid::layout(cx, b, inner);
        flow.first = r.children.is_empty();
        flow.cursor = r.height;
        flow.baseline = r.baseline.map(|v| v + origin.y);
        flow.children = r.children;
        flow.statics = r.statics;
        from_content(&mut flow, origin);
    } else if cx.tree.has_inline_children(b) {
        let r = inline::layout(cx, b, inner, flow.at);
        flow.first = r.empty;
        flow.cursor = r.height;
        flow.baseline = r.baseline.map(|v| v + origin.y);
        flow.children = r.lines;
        flow.statics = r.statics;
        from_content(&mut flow, origin);
    } else {
        children(cx, b, inner, origin, absorb_top, &mut mt, &mut flow);
    }
    let auto_h = h.is_none() && min_h == Au::ZERO;
    let absorb_bottom = !fc && e.bp(2) == Au::ZERO && auto_h;
    let mut mb = Strut::of(e.m(2));
    let mut content = if flow.first && absorb_top {
        mt.add(flow.strut);
        Au::ZERO
    } else if absorb_bottom {
        mb.add(flow.strut);
        flow.cursor
    } else {
        flow.cursor + flow.strut.resolve()
    };
    // floats raise the height as a minimum does: at that height as a fixed one, the contents would fill it
    let natural = content;
    if fc {
        let ex = cx.excl.pop().expect("pushed above");
        cx.counters.float_visits += ex.visits();
        // a formatting context root holds its floats
        content = content.max(ex.bottom() - origin.y);
    }
    let through = absorb_top && absorb_bottom && flow.first;
    let ch = h.unwrap_or(content).clamp_lo_hi(min_h, max_h);
    let size = Size {
        w: iw.w + e.bp_inline(),
        h: ch + e.bp_block(),
    };
    let node = if edges { cx.frag_node(b) } else { None };
    let mut kids = flow.children;
    let statics = position::place_absolute(cx, b, size, &e, &mut kids, flow.statics);
    Laid {
        frag: Fragment::with_scroller(FragKind::Box, node, size, kids, cx.scroller(b)),
        mt,
        mb,
        ml: iw.ml,
        mr: iw.mr,
        through,
        clamped: h.is_none() && ch != natural,
        baseline: flow.baseline,
        floats: false,
        statics,
    }
}

// flex and inline layout place from the content box: moved to the border box, as block flow places
fn from_content(flow: &mut Flow, origin: Point) {
    for c in &mut flow.children {
        c.offset = c.offset + origin;
    }
    for (_, p) in &mut flow.statics {
        *p = *p + origin;
    }
}

// a child that starts a formatting context goes beside the floats: the room it gets, and how far right
fn beside_floats(cx: &Cx, c: u32, flow: &Flow, y: Au, inner: Space) -> (Space, Au) {
    let Some(ex) = cx.excl.last().filter(|e| !e.is_empty()) else {
        return (inner, Au::ZERO);
    };
    let s = cx.style(c);
    if !cx.has_edges(c)
        || !new_formatting_context(s, false)
        || s.float != madcawl_css::style::Float::None
    {
        return (inner, Au::ZERO);
    }
    let left = flow.at.x;
    let (l, r) = ex.band(flow.at.y + y, Au(1), left, left + inner.width);
    let room = Space {
        width: (r - l).max(Au::ZERO),
        ..inner
    };
    (room, l - left)
}

// the block-level children of `b`, one under the other from `origin`, collapsing adjoining margins
fn children(
    cx: &mut Cx,
    b: u32,
    inner: Space,
    origin: Point,
    absorb_top: bool,
    mt: &mut Strut,
    flow: &mut Flow,
) {
    let kids: Vec<u32> = cx.tree.children(b).collect();
    for c in kids {
        let cs = cx.style(c);
        let edges = cx.has_edges(c);
        if edges && position::out_of_flow(cs) {
            // where it would be as a box of no height and no margins
            let y = if flow.first && absorb_top {
                flow.cursor
            } else {
                flow.cursor + flow.strut.resolve()
            };
            flow.statics.push((c, origin + Point { x: Au::ZERO, y }));
            continue;
        }
        let ce = if edges {
            cx.edges(cs, inner.width)
        } else {
            Edges::default()
        };
        let mut pending = flow.strut;
        pending.add_margin(ce.m(0));
        let mut ty = flow.cursor + pending.resolve();
        // clearance: below the floats it clears, and no collapsing across it
        if edges
            && let Some(ex) = cx.excl.last()
            && !ex.is_empty()
        {
            let cleared = ex.clear(flow.at.y + ty, cs.place.clear) - flow.at.y;
            if cleared > ty {
                flow.cursor = cleared - ce.m(0);
                flow.strut = Strut::default();
                flow.first = false;
                ty = cleared;
            }
        }
        let (room, dx) = beside_floats(cx, c, flow, ty, inner);
        let cat = flow.at
            + Point {
                x: ce.m(3) + dx,
                y: ty,
            };
        let r = layout_at(cx, c, room, cat);
        let rel = position::relative_offset(cs, inner, edges) + Point { x: dx, y: Au::ZERO };
        if r.through {
            flow.strut.add(r.mt);
            let y = flow.cursor + flow.strut.resolve();
            flow.strut.add(r.mb);
            let at = Point { x: r.ml, y } + origin + rel;
            flow.statics
                .extend(r.statics.iter().map(|&(a, p)| (a, p + at)));
            flow.children.push(Child {
                offset: at,
                frag: r.frag,
            });
            continue;
        }
        flow.strut.add(r.mt);
        let y = if flow.first && absorb_top {
            mt.add(flow.strut);
            flow.cursor
        } else {
            flow.cursor + flow.strut.resolve()
        };
        if let Some(bl) = r.baseline {
            flow.baseline = Some(origin.y + y + bl);
        }
        let at = Point { x: r.ml, y } + origin + rel;
        flow.statics
            .extend(r.statics.iter().map(|&(a, p)| (a, p + at)));
        flow.children.push(Child {
            offset: at,
            frag: r.frag.clone(),
        });
        flow.cursor = y + r.frag.size.h;
        flow.strut = r.mb;
        flow.first = false;
    }
}
