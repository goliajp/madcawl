//! A table as a Markdown pipe table: its rows (not those of tables inside
//! it), each row's cells by their names, the first row as the header.

use madcawl_dom::NodeId;

use super::Reader;
use super::markdown::{Block, TableRows};
use crate::role::Role;

fn cells(r: &Reader, row: NodeId) -> Vec<String> {
    r.dom
        .children(row)
        .filter_map(|c| r.writes(c))
        .filter(|rec| {
            matches!(
                rec.role,
                Role::Cell | Role::GridCell | Role::ColumnHeader | Role::RowHeader
            )
        })
        .map(|rec| {
            let mut s = String::new();
            super::md_text::push_md_text(&mut s, r.name(&rec));
            // after the Markdown escapes, so the backslash stays single
            s.replace('|', "\\|")
        })
        .collect()
}

/// The rows of `table`, in order, not descending into nested tables.
fn rows(r: &Reader, table: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack: Vec<NodeId> = r.dom.children(table).collect();
    stack.reverse();
    while let Some(n) = stack.pop() {
        let Some(rec) = r.writes(n) else { continue };
        match rec.role {
            Role::Row => out.push(n),
            Role::Table | Role::Grid | Role::TreeGrid => {}
            _ => {
                let mut kids: Vec<NodeId> = r.dom.children(n).collect();
                kids.reverse();
                stack.extend(kids);
            }
        }
    }
    out
}

fn row_text(cells: &[String], width: usize) -> String {
    let mut s = String::from("|");
    for i in 0..width {
        s.push(' ');
        s.push_str(cells.get(i).map_or("", |c| c.as_str()));
        s.push_str(" |");
    }
    s
}

pub(crate) fn table(r: &Reader, n: NodeId) -> Block {
    let rows: Vec<Vec<String>> = rows(r, n).into_iter().map(|row| cells(r, row)).collect();
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let reference = r
        .record(n)
        .map(|rec| r.reference(n, &rec).text())
        .unwrap_or_default();
    if width == 0 {
        return Block {
            text: String::new(),
            table: None,
            item: false,
        };
    }
    let mut head = row_text(&rows[0], width);
    head.push('\n');
    head.push('|');
    for _ in 0..width {
        head.push_str(" --- |");
    }
    let body: Vec<String> = rows[1..].iter().map(|c| row_text(c, width)).collect();
    let mut text = head.clone();
    for b in &body {
        text.push('\n');
        text.push_str(b);
    }
    Block {
        text,
        table: Some(TableRows {
            head,
            body,
            reference,
        }),
        item: false,
    }
}
