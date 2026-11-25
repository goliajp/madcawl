//! A table's structure: its captions, columns, row groups in display order (the first header group
//! first, the first footer group last), rows, and cells in the slots of the table grid with their spans.

use madcawl_css::style::Size as CssSize;
use madcawl_dom::NodeId;
use madcawl_html::Namespace;
use madcawl_str::{Atom, atom};

use crate::cx::Cx;
use crate::tree::NONE;

use super::anon::{Group, Role};
use super::role;

/// HTML's limits: a cell spans at most 1000 columns and 65534 rows.
pub const MAX_COLSPAN: usize = 1000;
pub const MAX_ROWSPAN: usize = 65534;
/// The most columns a table has: cells and column elements past it are put in the last one.
pub const MAX_COLUMNS: usize = 1 << 16;

pub struct Cell {
    pub b: u32,
    pub row: usize,
    pub col: usize,
    pub rows: usize,
    pub cols: usize,
}

pub struct RowGroup {
    /// `NONE` for rows directly in the table, which lay out as one group without a box
    pub b: u32,
    pub first: usize,
    pub end: usize,
}

#[derive(Default)]
pub struct Structure {
    pub captions: Vec<u32>,
    /// Column and column group boxes, which have no size
    pub column_boxes: Vec<u32>,
    /// By column, the width its column element gives
    pub col_widths: Vec<Option<CssSize>>,
    pub groups: Vec<RowGroup>,
    /// By row, its box
    pub rows: Vec<u32>,
    pub cells: Vec<Cell>,
    pub ncols: usize,
}

fn attr_span(cx: &Cx, n: NodeId, name: Atom, max: usize, zero: bool) -> usize {
    let html = cx.dom.namespace(n) == Some(Namespace::Html);
    let v = cx
        .dom
        .attribute(n, name)
        .and_then(|v| v.as_ascii_str())
        .map(|v| {
            v.trim_start_matches([' ', '\t', '\n', '\x0c', '\r'])
                .to_owned()
        });
    let Some(v) = v.filter(|_| html) else {
        return 1;
    };
    let digits: String = v.chars().take_while(char::is_ascii_digit).collect();
    match digits.parse::<u64>() {
        Ok(0) if zero => 0,
        Ok(0) | Err(_) => 1,
        Ok(k) => (k as usize).min(max),
    }
}

/// The spans of cell `b` from its `colspan` and `rowspan` attributes; a row span of 0 is to the end of
/// its group.
pub fn spans(cx: &Cx, b: u32) -> (usize, usize) {
    let Some(n) = cx.has_edges(b).then(|| cx.frag_node(b)).flatten() else {
        return (1, 1);
    };
    let is_cell = matches!(cx.dom.local_name(n), Some(a) if a == atom!("td") || a == atom!("th"));
    if !is_cell {
        return (1, 1);
    }
    (
        attr_span(cx, n, atom!("colspan"), MAX_COLSPAN, false),
        attr_span(cx, n, atom!("rowspan"), MAX_ROWSPAN, true),
    )
}

fn columns(cx: &Cx, b: u32, s: &mut Structure) {
    let width = |c: u32| Some(cx.style(c).boxm.width).filter(|w| !matches!(w, CssSize::Kw(_)));
    for c in cx.tree.children(b) {
        match role(cx, c) {
            Role::Column => {
                s.column_boxes.push(c);
                let n = attr_span(cx, cx.tree.get(c).node, atom!("span"), MAX_COLSPAN, false);
                extend(s, width(c), n);
            }
            Role::ColumnGroup => {
                s.column_boxes.push(c);
                let gw = width(c);
                let cols: Vec<u32> = cx
                    .tree
                    .children(c)
                    .filter(|&k| role(cx, k) == Role::Column)
                    .collect();
                if cols.is_empty() {
                    let n = attr_span(cx, cx.tree.get(c).node, atom!("span"), MAX_COLSPAN, false);
                    extend(s, gw, n);
                }
                for k in cols {
                    s.column_boxes.push(k);
                    let n = attr_span(cx, cx.tree.get(k).node, atom!("span"), MAX_COLSPAN, false);
                    extend(s, width(k).or(gw), n);
                }
            }
            _ => {}
        }
    }
}

fn extend(s: &mut Structure, w: Option<CssSize>, n: usize) {
    let n = n.min(MAX_COLUMNS - s.col_widths.len());
    s.col_widths.extend(std::iter::repeat_n(w, n));
}

// the rows of one group, each cell in the first free slot of its row
fn rows_of(cx: &Cx, rows: &[u32], s: &mut Structure, taken: &mut Vec<usize>) {
    let first = s.rows.len();
    let end = first + rows.len();
    taken.clear();
    for (k, &r) in rows.iter().enumerate() {
        let at = first + k;
        s.rows.push(r);
        let mut col = 0;
        for c in cx.tree.children(r) {
            if role(cx, c) != Role::Cell {
                continue;
            }
            while taken.get(col).is_some_and(|&until| until > at) {
                col += 1;
            }
            col = col.min(MAX_COLUMNS - 1);
            let (cs, rs) = spans(cx, c);
            let cs = cs.min(MAX_COLUMNS - col);
            let rs = if rs == 0 { end - at } else { rs.min(end - at) };
            if taken.len() < col + cs {
                taken.resize(col + cs, 0);
            }
            for t in &mut taken[col..col + cs] {
                *t = at + rs;
            }
            s.cells.push(Cell {
                b: c,
                row: at,
                col,
                rows: rs,
                cols: cs,
            });
            col += cs;
            s.ncols = s.ncols.max(col);
        }
    }
}

/// The structure of table `b`.
pub fn of(cx: &Cx, b: u32) -> Structure {
    let mut s = Structure::default();
    columns(cx, b, &mut s);
    let (mut head, mut foot, mut bodies) = (None, None, Vec::new());
    let mut loose: Vec<u32> = Vec::new();
    for c in cx.tree.children(b) {
        match role(cx, c) {
            Role::Caption => s.captions.push(c),
            Role::Row => loose.push(c),
            Role::Group(g) => {
                if !loose.is_empty() {
                    bodies.push((NONE, std::mem::take(&mut loose)));
                }
                let rows: Vec<u32> = cx
                    .tree
                    .children(c)
                    .filter(|&r| role(cx, r) == Role::Row)
                    .collect();
                match g {
                    Group::Head if head.is_none() => head = Some((c, rows)),
                    Group::Foot if foot.is_none() => foot = Some((c, rows)),
                    _ => bodies.push((c, rows)),
                }
            }
            _ => {}
        }
    }
    if !loose.is_empty() {
        bodies.push((NONE, loose));
    }
    let mut taken = Vec::new();
    for (g, rows) in head.into_iter().chain(bodies).chain(foot) {
        let first = s.rows.len();
        rows_of(cx, &rows, &mut s, &mut taken);
        s.groups.push(RowGroup {
            b: g,
            first,
            end: s.rows.len(),
        });
    }
    s.ncols = s.ncols.max(s.col_widths.len());
    effective_columns(&mut s);
    s
}

// columns no cell starts or ends between merge into one, as WebKit's effective columns: a cell spanning
// past every other adds no empty columns, nor the spacing between them
fn effective_columns(s: &mut Structure) {
    if s.cells.is_empty() {
        return;
    }
    let mut edge = vec![false; s.ncols + 1];
    edge[0] = true;
    for c in &s.cells {
        edge[c.col] = true;
        edge[c.col + c.cols] = true;
    }
    let last = s.cells.iter().map(|c| c.col + c.cols).max().unwrap_or(0);
    edge[last..].iter_mut().for_each(|e| *e = true);
    if edge.iter().all(|&e| e) {
        return;
    }
    // the effective column each column line starts
    let mut map = Vec::with_capacity(edge.len());
    let mut k = 0;
    for &e in &edge {
        if e && !map.is_empty() {
            k += 1;
        }
        map.push(k);
    }
    for c in &mut s.cells {
        let (a, b) = (map[c.col], map[c.col + c.cols]);
        (c.col, c.cols) = (a, b - a);
    }
    let mut widths = vec![None; k];
    for (i, w) in s.col_widths.iter().enumerate() {
        if edge[i] && map[i] < k {
            widths[map[i]] = *w;
        }
    }
    s.col_widths = widths;
    s.ncols = k;
}
