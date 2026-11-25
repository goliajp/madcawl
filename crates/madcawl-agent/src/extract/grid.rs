//! A table's cells laid on a grid the way HTML's table model places
//! them (`colspan`, `rowspan`, or `aria-colspan` and `aria-rowspan`
//! outside an HTML table), so a merged cell fills every slot it covers.
//! A grid of more than `MAX_SLOTS` slots is cut after the last row that
//! fits. A cell's text is read when a record or a key needs it.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use madcawl_dom::NodeId;
use madcawl_str::atom;

use super::{attr, shown_text};
use crate::html_table::is_html;
use crate::role::Role;
use crate::view::Reader;

/// The widest a row is read and the most rows a span reaches.
const MAX_COLS: usize = 1000;
const MAX_ROWSPAN: usize = 65_534;
/// The most grid slots a table is laid on.
pub(super) const MAX_SLOTS: usize = 1_000_000;

fn is_cell(role: Role) -> bool {
    matches!(
        role,
        Role::Cell | Role::GridCell | Role::ColumnHeader | Role::RowHeader
    )
}

/// A span attribute's number: digits at the start, else 1.
fn span(r: &Reader, n: NodeId, html: madcawl_str::Atom, aria: madcawl_str::Atom) -> usize {
    let v = attr(r.dom, n, html).or_else(|| attr(r.dom, n, aria));
    let digits: String = v
        .as_deref()
        .unwrap_or("")
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .take(6)
        .collect();
    digits.parse().unwrap_or(1)
}

struct Cell {
    node: NodeId,
    header: bool,
    text: OnceCell<Rc<str>>,
}

/// A cell reaching into the rows below the one it is in.
struct Span {
    x: usize,
    cols: usize,
    until: usize,
    cell: u32,
}

pub(super) type Line = Vec<Option<u32>>;

fn fill(line: &mut Line, x: usize, cols: usize, cell: u32) {
    if line.len() < x + cols {
        line.resize(x + cols, None);
    }
    line[x..x + cols].fill(Some(cell));
}

/// The cells of row `y` placed on `line`, after the spans from above.
fn place(
    r: &Reader,
    row: NodeId,
    y: usize,
    all: usize,
    g: &mut Grid,
    spans: &mut Vec<Span>,
) -> Line {
    let mut line = Line::new();
    for s in spans.iter() {
        fill(&mut line, s.x, s.cols, s.cell);
    }
    let mut x = 0;
    for c in r.dom.children(row) {
        let Some(rec) = r.writes(c).filter(|rec| is_cell(rec.role)) else {
            continue;
        };
        while line.get(x).is_some_and(Option::is_some) {
            x += 1;
        }
        if x >= MAX_COLS {
            break;
        }
        let cols = span(r, c, atom!("colspan"), atom!("aria-colspan")).clamp(1, MAX_COLS - x);
        let down = match span(r, c, atom!("rowspan"), atom!("aria-rowspan")) {
            // rowspan="0" reaches the end of the rows
            0 if is_html(r.dom, c, atom!("td")) || is_html(r.dom, c, atom!("th")) => all - y,
            0 => 1,
            d => d.min(MAX_ROWSPAN),
        };
        let at = g.cells.len() as u32;
        g.cells.push(Cell {
            node: c,
            header: rec.role == Role::ColumnHeader,
            text: OnceCell::new(),
        });
        fill(&mut line, x, cols, at);
        if down > 1 {
            spans.push(Span {
                x,
                cols,
                until: y + down,
                cell: at,
            });
        }
        x += cols;
    }
    line
}

/// The cells by row and column; `None` where nothing was placed.
pub(super) struct Grid {
    pub(super) lines: Vec<Line>,
    cells: Vec<Cell>,
    /// The rows laid before the slots ran out, when they did
    pub(super) cut: Option<usize>,
}

impl Grid {
    pub(super) fn new(r: &Reader, rows: &[NodeId]) -> Grid {
        let mut g = Grid {
            lines: Vec::new(),
            cells: Vec::new(),
            cut: None,
        };
        let mut spans: Vec<Span> = Vec::new();
        let mut slots = 0;
        for (y, &row) in rows.iter().enumerate() {
            spans.retain(|s| s.until > y);
            let line = place(r, row, y, rows.len(), &mut g, &mut spans);
            slots += line.len();
            if slots > MAX_SLOTS {
                g.cut = Some(y);
                break;
            }
            g.lines.push(line);
        }
        g
    }

    pub(super) fn width(&self) -> usize {
        self.lines.iter().map(Vec::len).max().unwrap_or(0)
    }

    pub(super) fn text(&self, r: &Reader, c: u32) -> &Rc<str> {
        let cell = &self.cells[c as usize];
        cell.text.get_or_init(|| shown_text(r, cell.node).into())
    }

    fn header(&self, c: Option<u32>) -> bool {
        c.is_some_and(|c| self.cells[c as usize].header)
    }

    /// The leading rows that are one header cell across the table:
    /// titles that name it.
    pub(super) fn title_rows(&self, width: usize) -> usize {
        self.lines
            .iter()
            .take_while(|line| {
                width > 1
                    && line.len() == width
                    && line.iter().all(|&c| c.is_some() && c == line[0])
                    && self.header(line[0])
            })
            .count()
    }

    /// The leading rows of header cells only; none when every row is.
    pub(super) fn head_rows(&self) -> usize {
        let head = self
            .lines
            .iter()
            .take_while(|line| {
                let mut placed = line.iter().flatten().peekable();
                placed.peek().is_some() && placed.all(|&c| self.cells[c as usize].header)
            })
            .count();
        if head == self.lines.len() { 0 } else { head }
    }

    /// The keys: the distinct header texts over each column, or `c1`,
    /// `c2`… where there are none; a repeated key gets ` (2)`, ` (3)`….
    pub(super) fn keys(&self, r: &Reader, head: usize, width: usize) -> Vec<Rc<str>> {
        let mut out: Vec<Rc<str>> = Vec::with_capacity(width);
        let mut taken: HashSet<Rc<str>> = HashSet::with_capacity(width);
        let mut next: HashMap<Rc<str>, usize> = HashMap::new();
        for x in 0..width {
            let mut parts: Vec<&str> = Vec::new();
            let mut seen: HashSet<&str> = HashSet::new();
            for line in &self.lines[..head] {
                if let Some(Some(c)) = line.get(x) {
                    let t: &str = self.text(r, *c);
                    if !t.is_empty() && seen.insert(t) {
                        parts.push(t);
                    }
                }
            }
            let base: Rc<str> = match parts.is_empty() {
                true => format!("c{}", x + 1).into(),
                false => parts.join(" / ").into(),
            };
            let mut k = base.clone();
            let n = next.entry(base.clone()).or_insert(1);
            while taken.contains(&k) {
                *n += 1;
                k = format!("{base} ({n})").into();
            }
            taken.insert(k.clone());
            out.push(k);
        }
        out
    }
}
