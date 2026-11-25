//! A table's rows as records keyed by its header, merged cells
//! expanded. The header is the leading rows that are all column headers
//! (a `thead`'s rows are); a column's key is the distinct header texts
//! above it joined by " / ". A table too big to lay out whole is cut,
//! and the records say so. Records are made one at a time, so listing
//! tables and paging read only what is shown.

use std::rc::Rc;

use madcawl_dom::NodeId;

use super::grid::{Grid, MAX_SLOTS};
use super::{Records, Row, Value, What, reference};
use crate::role::Role;
use crate::view::Reader;

fn is_table(role: Role) -> bool {
    matches!(role, Role::Table | Role::Grid | Role::TreeGrid)
}

/// The rows of `table` in order, not those of tables inside it.
fn row_nodes(r: &Reader, table: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack: Vec<NodeId> = r.dom.children(table).collect();
    stack.reverse();
    while let Some(n) = stack.pop() {
        let Some(rec) = r.writes(n) else { continue };
        match rec.role {
            Role::Row => out.push(n),
            role if is_table(role) => {}
            _ => {
                let mut kids: Vec<NodeId> = r.dom.children(n).collect();
                kids.reverse();
                stack.extend(kids);
            }
        }
    }
    out
}

/// A table laid out, its records made one at a time.
pub(super) struct Table {
    grid: Grid,
    rows: Vec<NodeId>,
    /// The grid lines that are records
    records: Vec<usize>,
    keys: Vec<Rc<str>>,
    pub(super) head: Records,
}

impl Table {
    /// The table `target` is (or is in), keyed by its header; `None`
    /// when there is none.
    pub(super) fn of(r: &Reader, target: NodeId) -> Option<Table> {
        let mut t = Some(target);
        while let Some(n) = t.filter(|&n| !r.record(n).is_some_and(|rec| is_table(rec.role))) {
            t = r.dom.parent(n);
        }
        Some(Table::laid(r, t?))
    }

    fn laid(r: &Reader, table: NodeId) -> Table {
        let mut head = Records::new(What::Table);
        head.source = Some(reference(r, table));
        head.label = r
            .record(table)
            .map(|rec| r.name(&rec).to_owned())
            .unwrap_or_default();
        let mut rows = row_nodes(r, table);
        let all = rows.len();
        let mut grid = Grid::new(r, &rows);
        let titles = grid.title_rows(grid.width());
        if titles > 0 {
            if head.label.is_empty() {
                let c = grid.lines[0][0].expect("a title cell");
                head.label = grid.text(r, c).to_string();
            }
            rows.drain(..titles);
            grid = Grid::new(r, &rows);
        }
        let width = grid.width();
        let hr = grid.head_rows();
        let keys = grid.keys(r, hr, width);
        head.columns = keys.iter().map(|k| k.to_string()).collect();
        if let Some(y) = grid.cut {
            head.note = format!("cut at row {} of {all}: over {MAX_SLOTS} cells", y + titles);
        }
        // a row with no cell of its own or spanning into it is no record
        let records = (hr..grid.lines.len())
            .filter(|&y| grid.lines[y].iter().any(Option::is_some))
            .collect();
        Table {
            grid,
            rows,
            records,
            keys,
            head,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.records.len()
    }

    pub(super) fn row(&self, r: &Reader, i: usize) -> Row {
        let y = self.records[i];
        let line = &self.grid.lines[y];
        let fields = self
            .keys
            .iter()
            .enumerate()
            .map(|(x, key)| {
                let v = match line.get(x).copied().flatten() {
                    Some(c) => self.grid.text(r, c).to_string(),
                    None => String::new(),
                };
                (key.clone(), Value::Text(v))
            })
            .collect();
        Row {
            node: self.rows[y],
            reference: reference(r, self.rows[y]),
            fields,
        }
    }
}

/// The rows of the table `target` is (or is in), every one of them.
pub(super) fn rows(r: &Reader, target: NodeId) -> Records {
    let Some(t) = Table::of(r, target) else {
        let mut recs = Records::new(What::Table);
        recs.label = "not a table, nor in one".into();
        return recs;
    };
    let rows = (0..t.len()).map(|i| t.row(r, i)).collect();
    Records { rows, ..t.head }
}

/// The tables in scope, one record each: its name, rows, columns and
/// header keys, so the caller can pick one by its reference.
pub(super) fn index(r: &Reader) -> Records {
    let mut recs = Records::new(What::Table);
    recs.label = "the tables; pass one's ref for its rows".into();
    recs.columns = ["name", "rows", "columns", "header"]
        .map(String::from)
        .to_vec();
    let root = r.root();
    for n in r.dom.descendants(root) {
        let Some(rec) = r.writes(n).filter(|rec| is_table(rec.role)) else {
            continue;
        };
        let t = Table::laid(r, n);
        let rows = match t.grid.cut {
            Some(_) => format!("{}+", t.len()),
            None => t.len().to_string(),
        };
        let text = |k: &str, v: String| (Rc::from(k), Value::Text(v));
        let fields = vec![
            text("name", r.name(&rec).to_owned()),
            text("rows", rows),
            text("columns", t.head.columns.len().to_string()),
            (Rc::from("header"), Value::List(t.head.columns)),
        ];
        recs.rows.push(Row {
            node: n,
            reference: reference(r, n),
            fields,
        });
    }
    recs
}
