//! Rows: each cell laid out at the width of its columns, row heights from the cells (baseline-aligned
//! cells by their baselines, cells spanning rows adding to their last row), and each cell then made as
//! tall as its rows with its content moved by `vertical-align`.

use std::sync::Arc;

use madcawl_css::style::{VAlign, VerticalAlign};

use crate::au::{Au, Point, Size};
use crate::block;
use crate::cx::{Cx, Space};
use crate::fragment::{Child, FragKind, Fragment};
use crate::style::{Edges, size};

use super::structure::Structure;

/// A cell laid out at its width, before its row gives it its height.
pub struct CellBox {
    pub kids: Vec<Child>,
    pub statics: Vec<(u32, Point)>,
    /// Its border-box width and the height its content needs
    pub w: Au,
    pub h: Au,
    /// From its border-box top
    pub baseline: Au,
    pub valign: Align,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Baseline,
    Top,
    Middle,
    Bottom,
}

fn valign(v: &VAlign) -> Align {
    match v {
        VAlign::Kw(VerticalAlign::Top) => Align::Top,
        VAlign::Kw(VerticalAlign::Middle) => Align::Middle,
        VAlign::Kw(VerticalAlign::Bottom) => Align::Bottom,
        _ => Align::Baseline,
    }
}

// the baseline of the first line: the last line's moved up by the lines between, when the lines are the
// cell's own
fn first_baseline(kids: &[Child], last: Option<Au>) -> Option<Au> {
    let last = last?;
    let lines: Vec<&Child> = kids
        .iter()
        .filter(|c| c.frag.kind == FragKind::Line)
        .collect();
    match (lines.first(), lines.last()) {
        (Some(a), Some(z)) if lines.len() > 1 => Some(last - (z.offset.y - a.offset.y)),
        _ => Some(last),
    }
}

/// Lays out cell `c` at border-box width `w`, its borders those of `borders` when the table collapses them.
pub fn cell(cx: &mut Cx, c: u32, w: Au, borders: Option<[Au; 4]>) -> CellBox {
    let s = cx.style(c);
    let e = if cx.has_edges(c) {
        Edges::of(s, w)
    } else {
        Edges::default()
    };
    let used = borders.unwrap_or(e.border);
    let bp_i = used[1] + used[3] + e.padding[1] + e.padding[3];
    let mut sp = Space::new(w, None);
    sp.fixed[0] = Some((w - bp_i).max(Au::ZERO));
    // its baseline sets its row's height: not measured
    let r = cx.measure(false, |cx| block::layout(cx, c, sp));
    // children move from the cell's own borders to the ones it is given
    let d = Point {
        x: used[3] - e.border[3],
        y: used[0] - e.border[0],
    };
    let kids: Vec<Child> = r
        .frag
        .children
        .iter()
        .map(|k| Child {
            offset: k.offset + d,
            frag: k.frag.clone(),
        })
        .collect();
    let bp_b = used[0] + used[2] + e.padding[0] + e.padding[2];
    let real_bp_b = e.bp_block();
    let mut content = r.frag.size.h - real_bp_b;
    if size(s.boxm.height, None).is_some() {
        // a cell's height is a minimum: its content may need more
        let below = kids
            .iter()
            .fold(Au::ZERO, |a, k| a.max(k.offset.y + k.frag.size.h));
        content = content.max(below - used[0] - e.padding[0]);
    }
    let h = content + bp_b;
    let baseline = first_baseline(&kids, r.baseline.map(|b| b + d.y))
        .unwrap_or(used[0] + e.padding[0] + content);
    CellBox {
        statics: r.statics.iter().map(|&(a, p)| (a, p + d)).collect(),
        kids,
        w,
        h,
        baseline,
        valign: if cx.has_edges(c) {
            valign(&s.rare.vertical_align)
        } else {
            Align::Baseline
        },
    }
}

/// The rows' heights and baselines from their cells.
pub struct Rows {
    pub heights: Vec<Au>,
    pub baselines: Vec<Option<Au>>,
    pub cells: Vec<CellBox>,
}

/// Lays every cell out at the `widths` of its columns, the spacing across and down between them, and
/// makes each row as tall as its cells need.
pub fn heights(
    cx: &mut Cx,
    st: &Structure,
    widths: &[Au],
    [hs, vs]: [Au; 2],
    borders: Option<&[[Au; 4]]>,
) -> Rows {
    let n = st.rows.len();
    let mut heights: Vec<Au> = st
        .rows
        .iter()
        .map(|&r| {
            let s = cx.style(r);
            let own = cx.has_edges(r).then(|| size(s.boxm.height, None)).flatten();
            own.unwrap_or(Au::ZERO).max(Au::ZERO)
        })
        .collect();
    let (mut asc, mut desc) = (vec![None::<Au>; n], vec![Au::ZERO; n]);
    let mut cells = Vec::with_capacity(st.cells.len());
    for (i, c) in st.cells.iter().enumerate() {
        let w = widths[c.col..c.col + c.cols]
            .iter()
            .fold(Au(hs.0.saturating_mul(c.cols as i32 - 1)), |a, &x| a + x);
        let cb = cell(cx, c.b, w, borders.map(|b| b[i]));
        if c.rows == 1 {
            if cb.valign == Align::Baseline {
                asc[c.row] = Some(asc[c.row].unwrap_or(Au::ZERO).max(cb.baseline));
                desc[c.row] = desc[c.row].max(cb.h - cb.baseline);
            }
            heights[c.row] = heights[c.row].max(cb.h);
        }
        cells.push(cb);
    }
    for r in 0..n {
        if let Some(a) = asc[r] {
            heights[r] = heights[r].max(a + desc[r]);
        }
    }
    // a cell spanning rows that need more adds what they lack to the last of them
    let mut spans: Vec<usize> = (0..st.cells.len())
        .filter(|&i| st.cells[i].rows > 1)
        .collect();
    spans.sort_by_key(|&i| st.cells[i].rows);
    for i in spans {
        let c = &st.cells[i];
        let last = c.row + c.rows - 1;
        let have = heights[c.row..=last]
            .iter()
            .fold(Au(vs.0.saturating_mul(c.rows as i32 - 1)), |a, &x| a + x);
        if cells[i].h > have {
            heights[last] += cells[i].h - have;
        }
    }
    Rows {
        heights,
        baselines: asc,
        cells,
    }
}

/// Spreads `extra` over the rows in proportion to their heights, equally when they have none.
pub fn grow(heights: &mut [Au], extra: Au) {
    if heights.is_empty() || extra <= Au::ZERO {
        return;
    }
    let total: i64 = heights.iter().map(|h| i64::from(h.0)).sum();
    let n = heights.len();
    let mut given = 0i64;
    for (k, h) in heights.iter_mut().enumerate() {
        let share = if k + 1 == n {
            i64::from(extra.0) - given
        } else if total > 0 {
            i64::from(extra.0) * i64::from(h.0) / total
        } else {
            i64::from(extra.0) / n as i64
        };
        given += share;
        *h += Au(share as i32);
    }
}

/// The fragment of a cell made `h` tall, its content moved down by its vertical alignment, and how far.
pub fn cell_fragment(
    cx: &Cx,
    b: u32,
    cb: &CellBox,
    h: Au,
    row_baseline: Option<Au>,
) -> (Arc<Fragment>, Au) {
    let dy = match cb.valign {
        Align::Top => Au::ZERO,
        Align::Middle => (h - cb.h).half(),
        Align::Bottom => h - cb.h,
        Align::Baseline => row_baseline.map_or(Au::ZERO, |rb| rb - cb.baseline),
    }
    .max(Au::ZERO);
    let kids = cb
        .kids
        .iter()
        .map(|k| Child {
            offset: k.offset + Point { x: Au::ZERO, y: dy },
            frag: k.frag.clone(),
        })
        .collect();
    let node = cx.has_edges(b).then(|| cx.frag_node(b)).flatten();
    (
        Fragment::new(FragKind::Box, node, Size { w: cb.w, h }, kids),
        dy,
    )
}
