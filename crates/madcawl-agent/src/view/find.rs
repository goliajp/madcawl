//! Find: the nodes whose name (or value) holds a text, of a role, or
//! both; one line each with its reference and where it is — the nearest
//! enclosing node with a role and a reference.

use madcawl_dom::{NodeId, NodeKind};

use super::snapshot::node_line;
use super::{Reader, View, finish};
use crate::refs::push_quoted;
use crate::role::Role;

#[derive(Clone, Debug, Default)]
pub struct Query {
    /// Text the name or value holds, ignoring case
    pub text: Option<String>,
    pub role: Option<Role>,
    /// The most results written; 0 for 20
    pub limit: usize,
}

/// Characters of context kept on each side of a match in text.
const CONTEXT: usize = 40;

pub(crate) fn write(r: &Reader, q: &Query) -> View {
    let limit = if q.limit == 0 { 20 } else { q.limit };
    let needle = q.text.as_deref().map(str::to_lowercase);
    let mut lines = Vec::new();
    let mut total = 0;
    let root = r.root();
    for n in std::iter::once(root).chain(r.dom.descendants(root)) {
        let Some(rec) = r.writes(n) else { continue };
        if rec.role.transparent() || rec.role == Role::Document {
            continue;
        }
        if q.role.is_some_and(|want| want != rec.role) {
            continue;
        }
        let name = r.name(&rec);
        let hit = match &needle {
            None => true,
            Some(t) => {
                name.to_lowercase().contains(t.as_str())
                    || r.tree.value(&rec).to_lowercase().contains(t.as_str())
            }
        };
        if !hit {
            continue;
        }
        total += 1;
        if lines.len() >= limit {
            continue;
        }
        let mut line = if r.dom.kind(n) == NodeKind::Text {
            let mut s = String::from("text ");
            push_quoted(&mut s, &snippet(name, needle.as_deref()));
            s.push(' ');
            r.push_ref(&mut s, n, &rec);
            s
        } else {
            node_line(r, n, &rec)
        };
        if let Some((p, prec)) = context(r, n) {
            line.push_str(" in ");
            line.push_str(prec.role.name());
            let pname = r.name(&prec);
            if !pname.is_empty() && r.dom.kind(n) != NodeKind::Text {
                line.push(' ');
                push_quoted(&mut line, &snippet(pname, None));
            }
            line.push(' ');
            r.push_ref(&mut line, p, &prec);
        }
        lines.push(line);
    }
    let mut text = format!("{total} found");
    if total > lines.len() {
        text.push_str(&format!(", first {} shown", lines.len()));
    }
    text.push('\n');
    for l in &lines {
        text.push_str(l);
        text.push('\n');
    }
    finish(text, total - lines.len())
}

/// The nearest ancestor with a role a reader sees.
fn context(r: &Reader, n: NodeId) -> Option<(NodeId, crate::record::Record)> {
    let mut up = r.dom.parent(n);
    while let Some(p) = up {
        if let Some(rec) = r.record(p)
            && !rec.role.transparent()
            && rec.role != Role::Document
        {
            return Some((p, rec));
        }
        up = r.dom.parent(p);
    }
    None
}

/// Up to `CONTEXT` characters each side of the match, or the start.
fn snippet(s: &str, needle: Option<&str>) -> String {
    let chars: Vec<char> = s.chars().collect();
    let at = needle
        .and_then(|t| {
            let lower: Vec<char> = s.to_lowercase().chars().collect();
            let t: Vec<char> = t.chars().collect();
            (0..lower.len()).find(|&i| lower[i..].starts_with(&t))
        })
        .filter(|_| s.to_lowercase().chars().count() == chars.len())
        .unwrap_or(0);
    let from = at.saturating_sub(CONTEXT);
    let to = (at + CONTEXT * 2).min(chars.len());
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    out.extend(&chars[from..to]);
    if to < chars.len() {
        out.push('…');
    }
    out
}
