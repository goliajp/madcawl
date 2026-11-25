//! Grid layout (CSS Grid 1, no subgrid): the explicit grid from the templates and areas, items placed by
//! their lines and the auto-placement algorithm, columns sized from the items' intrinsic widths, rows from
//! their heights at the columns' widths, tracks aligned in the content box, then each item laid out by
//! block layout in its grid area and aligned there.
//!
//! An item is measured before it is laid out: its intrinsic widths are kept per box, and its height for a
//! row is a layout at its column width with no definite height, one of the results each box keeps; the
//! final layout in its grid area is another, so neither pass repeats the other's work.

use madcawl_css::style::{ComputedStyle, Display, Lp};

use crate::au::{Au, Point};
use crate::cx::{Cx, Space};
use crate::fragment::Child;
use crate::position;

mod explicit;
mod flex;
mod items;
mod place;
mod span;
mod tracks;

pub use explicit::MAX_LINES;
use explicit::{Explicit, auto_repeat_count, explicit};
use place::{Item as Placed, Pos, auto_place, resolve};
use tracks::{Avail, Track};

/// What a grid container's content came to.
pub struct Gridded {
    pub children: Vec<Child>,
    /// Out-of-flow boxes below it, from the content box corner
    pub statics: Vec<(u32, Point)>,
    pub height: Au,
    pub baseline: Option<Au>,
}

pub fn is_grid(s: &ComputedStyle) -> bool {
    matches!(s.display, Display::Grid | Display::InlineGrid)
}

/// Whether `b` is an item of a grid container, which starts a formatting context of its own.
pub fn is_item(cx: &Cx, b: u32) -> bool {
    let p = cx.tree.get(b).parent;
    p != crate::tree::NONE && cx.has_edges(p) && is_grid(cx.style(p))
}

/// The computed values a grid item takes: an anonymous item has the initial ones.
fn item_style<'a>(cx: &Cx<'a>, c: u32) -> &'a ComputedStyle {
    if cx.has_edges(c) {
        cx.style(c)
    } else {
        ComputedStyle::initial()
    }
}

fn gap(g: Option<Lp>, base: Option<Au>) -> Au {
    g.map_or(Au::ZERO, |l| match base {
        _ if !l.has_pct() => Au(l.px),
        Some(b) => crate::style::lp(l, b),
        None => Au::ZERO,
    })
}

/// An item with its tracks, `first..end` on each axis.
struct GItem {
    b: u32,
    cols: (usize, usize),
    rows: (usize, usize),
}

/// The grid of a container: its tracks on both axes and its items placed among them.
struct Grid {
    cols: Vec<Track>,
    rows: Vec<Track>,
    items: Vec<GItem>,
    col_gap: Au,
    row_gap: Au,
    statics: Vec<(u32, Point)>,
}

fn tracks_of(ax: &Explicit, lo: i32, hi: i32, base: Option<Au>, used: &[bool]) -> Vec<Track> {
    (lo..hi)
        .map(|i| {
            let fit = ax
                .fit
                .is_some_and(|(a, b)| (a as i32..b as i32).contains(&i));
            if fit && !used[(i - lo) as usize] {
                Track::collapse()
            } else {
                Track::new(ax.size(i), base)
            }
        })
        .collect()
}

// the lines an axis spans, clamped so the axis has at most `MAX_LINES` tracks
fn bounds(explicit: i32, spans: impl Iterator<Item = (i32, i32)>) -> (i32, i32) {
    let (mut lo, mut hi) = (0, explicit);
    for (a, b) in spans {
        (lo, hi) = (lo.min(a), hi.max(b));
    }
    (lo, hi.min(lo.saturating_add(MAX_LINES)))
}

// `with_rows` false leaves the row tracks out, which sizing the columns alone does not read
fn setup(cx: &mut Cx, b: u32, width: Option<Au>, height: Option<Au>, with_rows: bool) -> Grid {
    let s = cx.style(b);
    let r = &s.rare;
    let (col_gap, row_gap) = (gap(r.column_gap, width), gap(r.row_gap, height));
    let areas = r.grid_template_areas.as_deref();
    let count = |t: &Option<_>, avail: Option<Au>, g: Au| {
        t.as_ref()
            .map_or(1, |t| auto_repeat_count(t, avail.map(|a| a.0), g.0))
    };
    let cols = explicit(
        r.grid_template_columns.as_ref(),
        areas,
        false,
        r.grid_auto_columns.clone(),
        count(&r.grid_template_columns, width, col_gap),
    );
    let rows = explicit(
        r.grid_template_rows.as_ref(),
        areas,
        true,
        r.grid_auto_rows.clone(),
        count(&r.grid_template_rows, height, row_gap),
    );
    let (mut statics, mut kids) = (Vec::new(), Vec::new());
    for c in cx.tree.children(b) {
        if cx.has_edges(c) && position::out_of_flow(cx.style(c)) {
            statics.push((c, Point::default()));
        } else {
            kids.push(c);
        }
    }
    kids.sort_by_key(|&c| item_style(cx, c).rare.order);
    let column_flow = r.grid_auto_flow.column;
    let mut placed: Vec<Placed> = kids
        .iter()
        .map(|&c| {
            let g = &item_style(cx, c).rare.grid_lines;
            let (row, col) = (resolve(&rows, &g[0], &g[2]), resolve(&cols, &g[1], &g[3]));
            if column_flow {
                Placed {
                    down: col,
                    across: row,
                }
            } else {
                Placed {
                    down: row,
                    across: col,
                }
            }
        })
        .collect();
    let across = if column_flow {
        rows.tracks()
    } else {
        cols.tracks()
    };
    auto_place(&mut placed, r.grid_auto_flow.dense, across);
    let lines = |p: Pos| match p {
        Pos::Lines(a, b) => (a, b),
        Pos::Auto(k) => (0, k),
    };
    let rc: Vec<((i32, i32), (i32, i32))> = placed
        .iter()
        .map(|p| {
            let (d, a) = (lines(p.down), lines(p.across));
            if column_flow { (a, d) } else { (d, a) }
        })
        .collect();
    let (r0, r1) = bounds(rows.tracks(), rc.iter().map(|x| x.0));
    let (c0, c1) = bounds(cols.tracks(), rc.iter().map(|x| x.1));
    let clip = |(a, b): (i32, i32), lo: i32, hi: i32| {
        let a = a.clamp(lo, hi - 1);
        ((a - lo) as usize, (b.clamp(a + 1, hi) - lo) as usize)
    };
    let mut used_r = vec![false; if with_rows { (r1 - r0) as usize } else { 0 }];
    let mut used_c = vec![false; (c1 - c0) as usize];
    let items: Vec<GItem> = kids
        .iter()
        .zip(&rc)
        .map(|(&c, &(rr, cc))| {
            let (rows, cols) = (clip(rr, r0, r1), clip(cc, c0, c1));
            if with_rows {
                used_r[rows.0..rows.1].iter_mut().for_each(|u| *u = true);
            }
            used_c[cols.0..cols.1].iter_mut().for_each(|u| *u = true);
            GItem { b: c, cols, rows }
        })
        .collect();
    Grid {
        cols: tracks_of(&cols, c0, c1, width, &used_c),
        rows: if with_rows {
            tracks_of(&rows, r0, r1, height, &used_r)
        } else {
            Vec::new()
        },
        items,
        col_gap,
        row_gap,
        statics,
    }
}

// a definite `min-height` of the content box, which `auto` rows stretch to fill when the height is not
// definite
fn min_height(s: &ComputedStyle, cw: Au) -> Option<Au> {
    let e = crate::style::Edges::of(s, cw);
    let v = crate::style::size(s.boxm.min_height, None)?;
    Some(crate::style::content_size(s, v, e.bp_block()))
}

/// Lays out the items of grid container `b` in its content box `inner`.
pub fn layout(cx: &mut Cx, b: u32, inner: Space) -> Gridded {
    let mut g = setup(cx, b, Some(inner.width), inner.height, true);
    let s = cx.style(b);
    let stretch = |a| items::content_stretches(a);
    let contribs = items::column_contribs(cx, &g, inner.width);
    tracks::size(
        &mut g.cols,
        &contribs,
        g.col_gap,
        Avail::Definite(inner.width),
        stretch(s.rare.justify_content).then_some(inner.width),
    );
    let cols = items::positions(&g.cols, g.col_gap, inner.width, s.rare.justify_content);
    let contribs = items::row_contribs(cx, b, &g, &cols);
    let avail = inner.height.map_or(Avail::MaxContent, Avail::Definite);
    let floor = inner.height.or_else(|| min_height(s, inner.width));
    tracks::size(
        &mut g.rows,
        &contribs,
        g.row_gap,
        avail,
        stretch(s.rare.align_content).then_some(floor).flatten(),
    );
    let content_h = items::span_size(&g.rows, g.row_gap, 0, g.rows.len());
    let rows = items::positions(
        &g.rows,
        g.row_gap,
        inner.height.unwrap_or(content_h),
        s.rare.align_content,
    );
    let (children, statics, baseline) = items::place(cx, b, &g, &cols, &rows);
    let mut all = std::mem::take(&mut g.statics);
    all.extend(statics);
    Gridded {
        children,
        statics: all,
        height: content_h,
        baseline,
    }
}

/// The min-content and max-content widths of a grid container's content: its columns sized under each
/// constraint.
pub fn content_widths(cx: &mut Cx, b: u32, cw: Au) -> (Au, Au) {
    let g = setup(cx, b, None, None, false);
    let contribs = items::column_contribs(cx, &g, cw);
    let mut out = [Au::ZERO; 2];
    for (k, avail) in [Avail::MinContent, Avail::MaxContent]
        .into_iter()
        .enumerate()
    {
        let mut ts = g.cols.clone();
        tracks::size(&mut ts, &contribs, g.col_gap, avail, None);
        out[k] = items::span_size(&ts, g.col_gap, 0, ts.len());
    }
    (out[0], out[1].max(out[0]))
}
