//! Data tables, row by row: each shown cell's content on one line,
//! cells separated by `|`. A table that holds tables, or says it is
//! only for presentation, is read as the blocks it contains.

use madcawl_dom::{NodeId, NodeKind};
use madcawl_html::Namespace;
use madcawl_str::{Atom, atom};

use super::text::{Line, quoted};
use super::{Item, Kind, Reader};

impl Reader<'_> {
    fn named(&self, n: NodeId, names: &[Atom]) -> bool {
        self.dom.kind(n) == NodeKind::Element
            && self.dom.namespace(n) == Some(Namespace::Html)
            && self.dom.local_name(n).is_some_and(|l| names.contains(&l))
    }

    fn layout_table(&self, el: NodeId) -> bool {
        let role = self.attr(el, atom!("role")).map(|r| r.to_ascii_lowercase());
        matches!(role.as_deref(), Some("presentation" | "none"))
            || self
                .dom
                .descendants(el)
                .any(|n| self.named(n, &[atom!("table")]))
    }

    fn rows(&self, el: NodeId) -> Vec<NodeId> {
        let groups = [atom!("thead"), atom!("tbody"), atom!("tfoot")];
        let mut out = Vec::new();
        for c in self.dom.children(el) {
            if self.named(c, &[atom!("tr")]) {
                out.push(c);
            } else if self.named(c, &groups) {
                out.extend(
                    self.dom
                        .children(c)
                        .filter(|&r| self.named(r, &[atom!("tr")])),
                );
            }
        }
        out
    }

    /// Reads a data table; false for a layout table, left to the caller.
    pub(crate) fn table(&mut self, el: NodeId, depth: u16) -> bool {
        if self.layout_table(el) {
            return false;
        }
        self.flush();
        self.next = None;
        let at = self.items.len();
        self.push(depth, Kind::Table, el, String::new());
        let mut cols = 0;
        let mut rows = 0;
        for r in self.rows(el) {
            if !self.shown.shown(r) {
                continue;
            }
            // a cell under visibility: hidden or opacity: 0 keeps its place,
            // so it is read as empty (or as what of it is visible again)
            let cells: Vec<NodeId> = self
                .dom
                .children(r)
                .filter(|&c| {
                    self.named(c, &[atom!("td"), atom!("th")])
                        && self.shown.styles.is_rendered(self.dom, c)
                })
                .collect();
            if cells.is_empty() {
                continue;
            }
            let head = cells.iter().all(|&c| self.named(c, &[atom!("th")]));
            let mut line = String::from("|");
            for &c in &cells {
                line.push(' ');
                line.push_str(&self.cell(c, depth + 1));
                line.push_str(" |");
            }
            cols = cols.max(cells.len());
            rows += 1;
            let kind = if head { Kind::HeadRow } else { Kind::Row };
            self.push(depth + 1, kind, r, line);
        }
        if rows == 0 {
            self.items.truncate(at);
            return true;
        }
        let mut text = format!("{rows}x{cols}");
        let caption = self
            .dom
            .children(el)
            .find(|&c| self.named(c, &[atom!("caption")]) && self.shown.shown(c));
        if let Some(c) = caption {
            let mut line = Line::default();
            self.flat(c, &mut line);
            if !line.s.is_empty() {
                text.push(' ');
                quoted(&mut text, &line.s);
            }
        }
        self.items[at].text = text;
        true
    }

    /// A cell's content read as items, then joined on one line.
    fn cell(&mut self, cell: NodeId, depth: u16) -> String {
        let at = self.items.len();
        self.walk(cell, depth, cell);
        self.flush();
        self.next = None;
        let parts: Vec<Item> = self.items.drain(at..).collect();
        let mut out = String::new();
        for p in parts {
            if !out.is_empty() {
                out.push(' ');
            }
            if let Kind::Control(role) = p.kind {
                out.push_str(role);
                out.push(' ');
            }
            for c in p.text.chars() {
                match c {
                    '|' => out.push_str("\\|"),
                    '\n' => out.push(' '),
                    c => out.push(c),
                }
            }
        }
        out
    }
}
