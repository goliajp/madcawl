//! The outline: the landmarks, each with how many interactive elements
//! it holds, and the headings, nested as the landmarks are. An agent
//! reads it first and then asks for the part it wants.

use madcawl_dom::NodeId;

use super::fold::{self, Line};
use super::snapshot::node_line;
use super::{Reader, View, finish};
use crate::refs::push_quoted;
use crate::role::Role;

/// The outline's budget when the caller gives none.
pub const OUTLINE_BUDGET: usize = 500;

struct Item {
    line: Line,
    /// A heading's level, 0 for a landmark
    level: u8,
    /// Interactive elements inside, for a landmark
    count: usize,
}

pub(crate) fn write(r: &Reader) -> View {
    let mut items: Vec<Item> = Vec::new();
    let mut total = 0;
    // (node, depth, entering); leaving a landmark closes its count
    let mut open: Vec<usize> = Vec::new();
    let mut stack = vec![(r.root(), 0u16, true)];
    while let Some((n, depth, entering)) = stack.pop() {
        if !entering {
            open.pop();
            continue;
        }
        let Some(rec) = r.record(n) else { continue };
        if rec.role == Role::None && n != r.dom.document() {
            continue;
        }
        let mut child_depth = depth;
        if let Some(rec) = r.writes(n) {
            if rec.role.interactive() {
                total += 1;
                if let Some(&k) = open.last() {
                    items[k].count += 1;
                }
            }
            if rec.role.landmark() || rec.role == Role::Heading {
                let landmark = rec.role != Role::Heading;
                items.push(Item {
                    line: Line {
                        depth,
                        text: node_line(r, n, &rec),
                        reference: Some(r.reference(n, &rec).text()),
                        many: false,
                    },
                    level: rec.level,
                    count: 0,
                });
                if landmark {
                    open.push(items.len() - 1);
                    stack.push((n, depth, false));
                    child_depth = depth + 1;
                }
            }
        }
        let kids: Vec<NodeId> = r.dom.children(n).collect();
        for &c in kids.iter().rev() {
            stack.push((c, child_depth, true));
        }
    }
    let mut lines = header(r, total);
    let budget = r.opts.budget.unwrap_or(OUTLINE_BUDGET);
    let (body, folded) = within(&items, budget.saturating_sub(cost(&lines[0])));
    lines.extend(body);
    let (lines, more) = fold::fit(lines, Some(budget));
    finish(fold::join(&lines), folded + more)
}

fn cost(l: &Line) -> usize {
    crate::text::estimate_tokens(&l.text) + l.depth as usize / 2 + 1
}

/// The landmarks, then the headings a level at a time from the top, each
/// level in document order, while they fit `budget`; and how many items
/// did not.
fn within(items: &[Item], budget: usize) -> (Vec<Line>, usize) {
    let lines: Vec<Line> = items.iter().map(item_line).collect();
    let mut keep = vec![false; items.len()];
    let mut used = 0;
    'levels: for level in 0..=u8::MAX {
        for (i, it) in items.iter().enumerate() {
            if it.level != level {
                continue;
            }
            let c = cost(&lines[i]);
            if used + c > budget && level != 0 {
                break 'levels;
            }
            used += c;
            keep[i] = true;
        }
        if !items.iter().any(|it| it.level > level) {
            break;
        }
    }
    let folded = keep.iter().filter(|k| !**k).count();
    let kept = lines
        .into_iter()
        .zip(keep)
        .filter_map(|(l, k)| k.then_some(l))
        .collect();
    (kept, folded)
}

fn header(r: &Reader, total: usize) -> Vec<Line> {
    let mut text = String::from("document");
    if let Some(t) = r.title() {
        text.push(' ');
        push_quoted(&mut text, &t);
    }
    text.push_str(&format!(" ({total} interactive)"));
    vec![Line {
        depth: 0,
        text,
        reference: None,
        many: false,
    }]
}

fn item_line(it: &Item) -> Line {
    let mut text = it.line.text.clone();
    if it.level == 0 {
        text.push_str(&format!(" ({} interactive)", it.count));
    }
    Line {
        depth: it.line.depth + 1,
        text,
        reference: it.line.reference.clone(),
        many: false,
    }
}
