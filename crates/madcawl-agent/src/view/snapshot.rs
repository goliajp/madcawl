//! The snapshot: an indented tree, one line a node, `role "name" [ref]
//! states`; static text on lines of its own. Nodes whose role adds
//! nothing are left out and their children take their place; text a
//! name already holds is not written again.

use madcawl_dom::{NodeId, NodeKind};

use super::fold::{self, Line};
use super::{Reader, View, finish};
use crate::record::{NAME_ATTR, NAME_CONTENT, PASSWORD, Record, STATE_WORDS, VISIBLE};
use crate::refs::{push_page_text, push_quoted};
use crate::role::Role;

pub(crate) fn write(r: &Reader) -> View {
    let mut lines = Vec::new();
    if r.opts.scope == super::Scope::Page
        && let Some(t) = r.title()
    {
        let mut text = String::from("document ");
        push_quoted(&mut text, &t);
        lines.push(Line {
            depth: 0,
            text,
            reference: None,
            many: false,
        });
    }
    walk(r, r.root(), &mut lines);
    let (lines, folded) = fold::fit(lines, r.opts.budget);
    finish(fold::join(&lines), folded)
}

/// The words after a node's reference: its states, level and value.
pub(crate) fn push_states(r: &Reader, out: &mut String, rec: &Record) {
    for (f, w) in STATE_WORDS {
        if rec.has(f) {
            out.push(' ');
            out.push_str(w);
        }
    }
    if rec.level != 0 {
        out.push_str(" level=");
        out.push_str(&rec.level.to_string());
    }
    if rec.value != 0 && rec.role != Role::Link {
        out.push_str(" value=");
        if rec.has(PASSWORD) {
            out.push_str(crate::states::MASK);
        } else {
            push_quoted(out, r.tree.value(rec));
        }
    }
    if rec.has(NAME_ATTR) {
        out.push_str(" (name from attribute, text differs)");
    }
    if !rec.has(VISIBLE) {
        out.push_str(" hidden");
    }
}

/// One node's line: `role "name" [ref] states`.
pub(crate) fn node_line(r: &Reader, n: NodeId, rec: &Record) -> String {
    let mut text = String::from(rec.role.name());
    let name = r.name(rec);
    if !name.is_empty() {
        text.push(' ');
        push_quoted(&mut text, name);
    }
    text.push(' ');
    r.push_ref(&mut text, n, rec);
    push_states(r, &mut text, rec);
    text
}

fn walk(r: &Reader, root: NodeId, lines: &mut Vec<Line>) {
    let mut stack = vec![(root, 0u16, false)];
    while let Some((n, depth, covered)) = stack.pop() {
        let Some(rec) = r.record(n) else { continue };
        if rec.role == Role::None && n != r.dom.document() {
            continue;
        }
        let mut child_depth = depth;
        let mut child_covered = covered;
        if let Some(rec) = r.writes(n) {
            if r.dom.kind(n) == NodeKind::Text {
                let name = r.name(&rec);
                if !covered && !name.is_empty() {
                    let mut text = String::new();
                    push_page_text(&mut text, name);
                    lines.push(Line {
                        depth,
                        text,
                        reference: None,
                        many: false,
                    });
                }
            } else if !rec.role.transparent() && rec.role != Role::Document {
                lines.push(Line {
                    depth,
                    text: node_line(r, n, &rec),
                    reference: Some(r.reference(n, &rec).text()),
                    many: rec.role.many_children(),
                });
                child_depth = depth + 1;
                child_covered = covered || rec.has(NAME_CONTENT) && rec.name != 0;
            }
        }
        let kids: Vec<NodeId> = r.dom.children(n).collect();
        for &c in kids.iter().rev() {
            stack.push((c, child_depth, child_covered));
        }
    }
}
