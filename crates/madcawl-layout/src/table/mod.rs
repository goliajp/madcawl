//! Table layout (CSS 2 17): the anonymous table objects (`anon`), the table grid of rows and cells with
//! their spans (`structure`), column widths by the automatic or the fixed table layout (`widths`), row
//! heights and cells aligned in them (`rows`), and the separated or collapsing border model (`collapse`).
//! A table's box holds its captions, above or below its row groups; row groups hold their rows and rows
//! their cells, a cell spanning rows hanging below the first of them.

use madcawl_css::style::{BorderCollapse, BoxSizing, CaptionSide, TableLayout};

use crate::au::{Au, Point, Size};
use crate::block;
use crate::cx::{Cx, Laid, Space, Strut};
use crate::fragment::{Child, FragKind, Fragment};
use crate::sizing::margins_for;
use crate::style::{Edges, size};
use crate::tree::BoxTree;
use crate::{intrinsic, position};

mod anon;
mod collapse;
mod distribute;
mod place;
mod rows;
mod structure;
mod widths;

use anon::Role;
pub use anon::Roles;
use structure::Structure;

/// Adds the anonymous table boxes to a box tree just built.
pub fn fix_tree(t: &mut BoxTree, styles: &madcawl_css::style::Styles) {
    anon::fix(t, styles);
}

fn role(cx: &Cx, b: u32) -> Role {
    anon::role(cx.tree, b, |x| anon::display(cx.tree, cx.styles, x))
}

/// Whether `b` is a table box, an element's or an anonymous one.
pub fn is_table(cx: &Cx, b: u32) -> bool {
    role(cx, b) == Role::Table
}

/// What a table's grid is laid out with: its structure, spacing, the borders and padding around it and
/// the collapsed borders of its cells.
struct Setup {
    st: Structure,
    spacing: [Au; 2],
    e: Edges,
    cells: Option<Vec<[Au; 4]>>,
    fixed: bool,
}

fn setup(cx: &mut Cx, b: u32, cw: Au) -> Setup {
    let st = structure::of(cx, b);
    let s = cx.style(b);
    let edges = cx.has_edges(b);
    let mut e = if edges {
        Edges::of(s, cw)
    } else {
        Edges::default()
    };
    let collapsing = s.text.border_collapse == BorderCollapse::Collapse;
    let (spacing, cells) = if collapsing {
        let c = collapse::resolve(cx, b, &st);
        (e.border, e.padding) = (c.table, [Au::ZERO; 4]);
        ([Au::ZERO; 2], Some(c.cells))
    } else {
        let sp = s.text.border_spacing;
        ([Au(sp[0]), Au(sp[1])], None)
    };
    let fixed = edges
        && s.rare.table_layout == TableLayout::Fixed
        && size(s.boxm.width, Some(cw)).is_some();
    Setup {
        st,
        spacing,
        e,
        cells,
        fixed,
    }
}

// the borders and padding across of the `i`th cell of the structure, as it lays out
fn cell_bp(cx: &Cx, i: usize, cells: &Option<Vec<[Au; 4]>>, st: &Structure) -> Au {
    let c = st.cells[i].b;
    let e = if cx.has_edges(c) {
        Edges::of(cx.style(c), Au::ZERO)
    } else {
        Edges::default()
    };
    let border = cells.as_ref().map_or(e.border, |v| v[i]);
    border[1] + border[3] + e.padding[1] + e.padding[3]
}

fn columns(cx: &mut Cx, su: &Setup) -> Vec<widths::Col> {
    let (cells, st) = (&su.cells, &su.st);
    widths::columns(cx, st, su.spacing[0], &|cx, c| cell_bp(cx, c, cells, st))
}

/// The min-content and max-content widths of table `b`'s content box.
pub fn content_widths(cx: &mut Cx, b: u32, cw: Au) -> (Au, Au) {
    let su = setup(cx, b, cw);
    let cols = columns(cx, &su);
    let (lo, hi) = widths::total(&cols, su.spacing[0]);
    let cap = su.st.captions.iter().fold(Au::ZERO, |a, &c| {
        a.max(intrinsic::margin_box_widths(cx, c, cw).0)
    });
    // border-box widths, less the borders and padding the caller adds back
    let bp = su.e.bp_inline();
    let (lo, hi) = ((lo + bp).max(cap), (hi + bp).max(cap));
    let real = if cx.has_edges(b) {
        Edges::of(cx.style(b), Au::ZERO).bp_inline()
    } else {
        Au::ZERO
    };
    ((lo - real).max(Au::ZERO), (hi.max(lo) - real).max(Au::ZERO))
}

// the table's border-box width: what it asks, never less than its columns need
fn width(cx: &mut Cx, b: u32, space: Space, su: &Setup, min: Au, max: Au) -> Au {
    let bp = su.e.bp_inline();
    if let Some(w) = space.fixed[0] {
        return (w + bp).max(min);
    }
    if !cx.has_edges(b) {
        return (space.width).min(max).max(min);
    }
    let s = cx.style(b);
    let e = Edges::of(s, space.width);
    let bb = |v: Au| match s.boxm.box_sizing {
        BoxSizing::BorderBox => v.max(bp),
        BoxSizing::ContentBox => v + bp,
    };
    let w = match size(s.boxm.width, Some(space.width)) {
        Some(v) => bb(v),
        None => (space.width - e.m(1) - e.m(3)).min(max),
    };
    let lo = size(s.boxm.min_width, Some(space.width)).map_or(Au::ZERO, bb);
    let hi = size(s.boxm.max_width, Some(space.width)).map_or(Au::MAX, bb);
    w.clamp_lo_hi(lo, hi).max(min)
}

/// Lays out table `b` in `space`: its captions, row groups, rows and cells.
pub fn layout(cx: &mut Cx, b: u32, space: Space) -> Laid {
    let su = setup(cx, b, space.width);
    let st = &su.st;
    let [hs, vs] = su.spacing;
    let cols = columns(cx, &su);
    let (min, max) = widths::total(&cols, hs);
    let cap_min = st.captions.iter().fold(Au::ZERO, |a, &c| {
        a.max(intrinsic::margin_box_widths(cx, c, space.width).0)
    });
    let bp = su.e.bp_inline();
    // the fixed layout's columns need only what the first row and the column elements ask
    let min = if su.fixed {
        let (cells, st2) = (&su.cells, &su.st);
        let ws = widths::fixed(cx, st2, Au::ZERO, &|cx, c| cell_bp(cx, c, cells, st2));
        let n = ws.len() as i32;
        ws.iter()
            .fold(Au(hs.0.saturating_mul(n + i32::from(n > 0))), |a, &x| a + x)
    } else {
        min
    };
    let w = width(cx, b, space, &su, (min + bp).max(cap_min), max + bp);
    let gaps = Au(hs
        .0
        .saturating_mul(cols.len() as i32 + i32::from(!cols.is_empty())));
    let avail = w - bp - gaps;
    let col_w = if su.fixed {
        let (cells, st2) = (&su.cells, &su.st);
        widths::fixed(cx, st2, avail, &|cx, c| cell_bp(cx, c, cells, st2))
    } else {
        distribute::distribute(&cols, avail)
    };
    let mut r = rows::heights(cx, st, &col_w, su.spacing, su.cells.as_deref());
    let grid_h = |h: &[Au]| {
        let n = h.len() as i32;
        h.iter()
            .fold(Au(vs.0.saturating_mul(n + i32::from(n > 0))), |a, &x| a + x)
    };
    let s = cx.style(b);
    let e_real = if cx.has_edges(b) {
        Edges::of(s, space.width)
    } else {
        Edges::default()
    };
    let own_h = cx
        .has_edges(b)
        .then(|| size(s.boxm.height, space.height))
        .flatten()
        .map(|v| match s.boxm.box_sizing {
            BoxSizing::BorderBox => v - su.e.bp_block(),
            BoxSizing::ContentBox => v,
        });
    if let Some(h) = space.fixed[1].or(own_h) {
        let extra = h - grid_h(&r.heights);
        rows::grow(&mut r.heights, extra);
    }
    let mut kids = Vec::new();
    let mut statics = Vec::new();
    let mut y = Au::ZERO;
    let caption = |cx: &mut Cx,
                   c: u32,
                   y: &mut Au,
                   kids: &mut Vec<Child>,
                   statics: &mut Vec<(u32, Point)>| {
        let l = block::layout(cx, c, Space::new(w, None));
        let at = Point {
            x: l.ml,
            y: *y + l.mt.resolve(),
        };
        statics.extend(l.statics.iter().map(|&(a, p)| (a, p + at)));
        *y = at.y + l.frag.size.h + l.mb.resolve();
        kids.push(Child {
            offset: at,
            frag: l.frag,
        });
    };
    let (top_caps, bottom_caps): (Vec<u32>, Vec<u32>) = st
        .captions
        .iter()
        .partition(|&&c| cx.style(c).text.caption_side == CaptionSide::Top);
    for c in top_caps {
        caption(cx, c, &mut y, &mut kids, &mut statics);
    }
    let top = y;
    for &c in &st.column_boxes {
        kids.push(Child {
            offset: Point::default(),
            frag: Fragment::new(
                FragKind::Box,
                cx.frag_node(c),
                Size::default(),
                Vec::new(),
            ),
        });
    }
    let origin = Point {
        x: su.e.bp(3),
        y: top + su.e.bp(0),
    };
    let inner_w = w - bp;
    let body_h = place::grid(
        cx,
        &su,
        &r,
        &col_w,
        inner_w,
        origin,
        &mut kids,
        &mut statics,
    );
    let content_h = body_h.max(space.fixed[1].or(own_h).unwrap_or(Au::ZERO));
    y = origin.y + content_h + su.e.bp(2);
    for c in bottom_caps {
        caption(cx, c, &mut y, &mut kids, &mut statics);
    }
    let size_b = Size { w, h: y };
    let baseline = r
        .baselines
        .first()
        .copied()
        .flatten()
        .map(|v| origin.y + vs + v);
    let m = margins_for(w - e_real.bp_inline(), space.width, &e_real);
    let statics = position::place_absolute(cx, b, size_b, &su.e, &mut kids, statics);
    Laid {
        frag: Fragment::new(
            FragKind::Box,
            cx.has_edges(b).then(|| cx.frag_node(b)).flatten(),
            size_b,
            kids,
        ),
        mt: Strut::of(e_real.m(0)),
        mb: Strut::of(e_real.m(2)),
        ml: m.ml,
        mr: m.mr,
        through: false,
        baseline,
        clamped: false,
        floats: false,
        statics,
    }
}
