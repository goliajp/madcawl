//! The collapsing border model (CSS 2 17.6.2): each edge between cells takes the widest of the borders
//! meeting there (the cells', their rows' and row groups' where the edge is theirs, the table's at its
//! outer edges), none at all when one of them is `hidden`; a cell lays out with half of each of its edges
//! and the table with half of its outer ones, split as WebKit splits them: in whole device pixels at the
//! layout's scale, the odd one to the side WebKit gives it.

use madcawl_css::style::BorderStyle;

use crate::au::Au;
use crate::cx::Cx;
use crate::tree::NONE;

use super::structure::Structure;

/// The most slots a table grid may have for its borders to be resolved against their neighbours; past it,
/// each cell's edges are its own and the table's.
pub const MAX_SLOTS: usize = 1 << 22;

pub struct Collapsed {
    /// By cell, the borders it lays out with: top, right, bottom, left
    pub cells: Vec<[Au; 4]>,
    pub table: [Au; 4],
}

/// One border meeting at an edge: its width, or `None` when it is `hidden`.
type Side = Option<Au>;

fn half_down(w: Au, scale: f32) -> Au {
    Au::from_px((w.px() * scale / 2.0).floor() / scale)
}

fn half_up(w: Au, scale: f32) -> Au {
    Au::from_px(((w.px() * scale + 1.0) / 2.0).floor() / scale)
}

fn own(cx: &Cx, b: u32) -> [Side; 4] {
    if b == NONE || !cx.has_edges(b) {
        return [Some(Au::ZERO); 4];
    }
    let s = cx.style(b);
    let w = cx.edges(s, Au::ZERO).border;
    std::array::from_fn(|k| (s.paint.border_style[k] != BorderStyle::Hidden).then_some(w[k]))
}

// the widest, or none when one is hidden
fn meet(a: Side, b: Side) -> Side {
    Some(a?.max(b?))
}

/// The borders of the cells of `st` and of table `b`, collapsed.
pub fn resolve(cx: &Cx, b: u32, st: &Structure) -> Collapsed {
    let t = own(cx, b);
    let (nr, nc) = (st.rows.len(), st.ncols);
    let mine: Vec<[Side; 4]> = st.cells.iter().map(|c| own(cx, c.b)).collect();
    let rows: Vec<[Side; 4]> = st.rows.iter().map(|&r| own(cx, r)).collect();
    let mut group_of = vec![0usize; nr];
    let groups: Vec<[Side; 4]> = st.groups.iter().map(|g| own(cx, g.b)).collect();
    for (k, g) in st.groups.iter().enumerate() {
        group_of[g.first..g.end].iter_mut().for_each(|x| *x = k);
    }
    let first_of = |r: usize| st.groups[group_of[r]].first == r;
    let last_of = |r: usize| st.groups[group_of[r]].end == r + 1;
    let mut slot = Vec::new();
    if nr.saturating_mul(nc) <= MAX_SLOTS {
        slot = vec![u32::MAX; nr * nc];
        for (i, c) in st.cells.iter().enumerate() {
            for r in c.row..c.row + c.rows {
                slot[r * nc + c.col..r * nc + c.col + c.cols].fill(i as u32);
            }
        }
    }
    let at = |r: usize, k: usize| slot.get(r * nc + k).copied().filter(|&i| i != u32::MAX);
    let nb = |r: usize, k: usize, side: usize| {
        at(r, k).map_or(Some(Au::ZERO), |j| mine[j as usize][side])
    };
    let mut edge = vec![[Au::ZERO; 4]; st.cells.len()];
    let mut outer: [Side; 4] = t;
    for (i, c) in st.cells.iter().enumerate() {
        let mut w = mine[i];
        let (r1, k1, last) = (c.row + c.rows, c.col + c.cols, c.row + c.rows - 1);
        // the horizontal edges also belong to the rows and row groups on either side of them
        let line = |r: usize| -> Side {
            let mut v = meet(rows[r - 1][2], rows[r][0]);
            if first_of(r) {
                v = meet(v, meet(groups[group_of[r - 1]][2], groups[group_of[r]][0]));
            }
            v
        };
        w[0] = if c.row == 0 {
            let g = if first_of(0) {
                groups[group_of[0]][0]
            } else {
                Some(Au::ZERO)
            };
            meet(w[0], meet(rows[0][0], g))
        } else {
            meet(w[0], line(c.row))
        };
        w[2] = if r1 == nr {
            let g = if last_of(last) {
                groups[group_of[last]][2]
            } else {
                Some(Au::ZERO)
            };
            meet(w[2], meet(rows[last][2], g))
        } else {
            meet(w[2], line(r1))
        };
        for k in c.col..k1 {
            w[0] = meet(
                w[0],
                if c.row == 0 {
                    t[0]
                } else {
                    nb(c.row - 1, k, 2)
                },
            );
            w[2] = meet(w[2], if r1 == nr { t[2] } else { nb(r1, k, 0) });
        }
        for r in c.row..r1 {
            let (rw, gw) = (rows[r], groups[group_of[r]]);
            let left = if c.col == 0 {
                meet(t[3], meet(rw[3], gw[3]))
            } else {
                nb(r, c.col - 1, 1)
            };
            let right = if k1 == nc {
                meet(t[1], meet(rw[1], gw[1]))
            } else {
                nb(r, k1, 3)
            };
            w[3] = meet(w[3], left);
            w[1] = meet(w[1], right);
        }
        // the table's left and right halves are those of its first row's edges (css 2 17.6.2)
        let outer_edge = [c.row == 0, k1 == nc && c.row == 0, r1 == nr, c.col == 0 && c.row == 0];
        for k in 0..4 {
            if outer_edge[k] {
                outer[k] = outer[k].and_then(|o| w[k].map(|x| o.max(x)).or(Some(o)));
            }
        }
        edge[i] = w.map(|x| x.unwrap_or(Au::ZERO));
    }
    let outer = outer.map(|x| x.unwrap_or(Au::ZERO));
    let (half_up, half_down) = (|w| half_up(w, cx.scale), |w| half_down(w, cx.scale));
    Collapsed {
        cells: edge
            .iter()
            .map(|w| {
                [
                    half_up(w[0]),
                    half_down(w[1]),
                    half_down(w[2]),
                    half_up(w[3]),
                ]
            })
            .collect(),
        table: [
            half_down(outer[0]),
            half_up(outer[1]),
            half_up(outer[2]),
            half_down(outer[3]),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edge_splits_with_the_odd_pixel_on_the_side_webkit_gives_it() {
        let px = |v: f32| Au::from_px(v);
        assert_eq!(
            [1.0, 2.0, 3.0, 5.0].map(|v| half_up(px(v), 1.0).px()),
            [1.0, 1.0, 2.0, 3.0]
        );
        assert_eq!(
            [1.0, 2.0, 3.0, 5.0].map(|v| half_down(px(v), 1.0).px()),
            [0.0, 1.0, 1.0, 2.0]
        );
        // at 2x a half is a whole device pixel
        assert_eq!(
            [1.0, 3.0, 1.5].map(|v| half_up(px(v), 2.0).px()),
            [0.5, 1.5, 1.0]
        );
        assert_eq!(
            [1.0, 3.0, 1.5].map(|v| half_down(px(v), 2.0).px()),
            [0.5, 1.5, 0.5]
        );
        assert_eq!(meet(Some(px(1.0)), Some(px(3.0))), Some(px(3.0)));
        assert_eq!(meet(Some(px(9.0)), None), None, "hidden wins");
    }
}
