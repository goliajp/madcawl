//! Placing a table's row groups, rows and cells once their sizes are known: rows one under the other with
//! the spacing between them, each group as tall as its rows, each cell over the columns and rows it spans.

use crate::au::{Au, Point, Size};
use crate::cx::Cx;
use crate::fragment::{Child, FragKind, Fragment};
use crate::tree::NONE;

use super::{Setup, rows};

// the row groups, rows and cells from `origin`; returns the height they take
#[allow(clippy::too_many_arguments)]
pub(super) fn grid(
    cx: &mut Cx,
    su: &Setup,
    r: &rows::Rows,
    col_w: &[Au],
    inner_w: Au,
    origin: Point,
    kids: &mut Vec<Child>,
    statics: &mut Vec<(u32, Point)>,
) -> Au {
    let st = &su.st;
    let [hs, vs] = su.spacing;
    let mut col_x = Vec::with_capacity(col_w.len() + 1);
    let mut x = hs;
    for &cw in col_w {
        col_x.push(x);
        x += cw + hs;
    }
    col_x.push(x);
    let mut row_y = Vec::with_capacity(st.rows.len());
    let mut y = if st.rows.is_empty() { Au::ZERO } else { vs };
    for &h in &r.heights {
        row_y.push(y);
        y += h + vs;
    }
    let bottom = y;
    let mut by_row: Vec<Vec<usize>> = vec![Vec::new(); st.rows.len()];
    for (i, c) in st.cells.iter().enumerate() {
        by_row[c.row].push(i);
    }
    let mut g_top = Au::ZERO;
    for (gi, g) in st.groups.iter().enumerate() {
        let g_bottom = if g.end > g.first {
            row_y[g.end - 1] + r.heights[g.end - 1] + vs
        } else {
            g_top
        };
        let g_at = origin
            + Point {
                x: Au::ZERO,
                y: g_top,
            };
        let mut rows_out = Vec::with_capacity(g.end - g.first);
        for ri in g.first..g.end {
            let ry = row_y[ri];
            let mut cells_out = Vec::with_capacity(by_row[ri].len());
            for &i in &by_row[ri] {
                let c = &st.cells[i];
                let last = c.row + c.rows - 1;
                let h = row_y[last] + r.heights[last] - ry;
                let rb = r.baselines[c.row];
                let (frag, dy) = rows::cell_fragment(cx, c.b, &r.cells[i], h, rb);
                let at = Point {
                    x: col_x[c.col],
                    y: Au::ZERO,
                };
                let abs =
                    g_at + Point {
                        x: Au::ZERO,
                        y: ry - g_top,
                    } + at
                        + Point { x: Au::ZERO, y: dy };
                statics.extend(r.cells[i].statics.iter().map(|&(a, p)| (a, p + abs)));
                cells_out.push(Child { offset: at, frag });
            }
            let rb = st.rows[ri];
            let node = cx.has_edges(rb).then(|| cx.frag_node(rb)).flatten();
            let frag = Fragment::new(
                FragKind::Box,
                node,
                Size {
                    w: inner_w,
                    h: r.heights[ri],
                },
                cells_out,
            );
            rows_out.push(Child {
                offset: Point {
                    x: Au::ZERO,
                    y: ry - g_top,
                },
                frag,
            });
        }
        let node = (g.b != NONE && cx.has_edges(g.b)).then(|| cx.frag_node(g.b)).flatten();
        let h = if gi + 1 == st.groups.len() {
            bottom - g_top
        } else {
            g_bottom - g_top
        };
        kids.push(Child {
            offset: g_at,
            frag: Fragment::new(FragKind::Box, node, Size { w: inner_w, h }, rows_out),
        });
        g_top += h;
    }
    bottom.max(g_top)
}
