//! Repeated structures as records. Siblings are of a kind when they have
//! the same element name, role and class list. Given an item ("the same
//! kind as this"), the items are the kind of whichever of it and its
//! ancestors has the most siblings of its own kind; with none, the kind
//! with the most text (the median item's, times the items) among the
//! sibling groups in scope, out of navigation, banners and footers. When the items of a kind are
//! each followed or preceded by the same run of other siblings (a `dt`
//! and its `dd`, a row and the row of details under it), the run belongs
//! to the item, and the record starts at the first of them.

use std::collections::HashMap;

use madcawl_dom::{NodeId, NodeKind};
use madcawl_str::atom;

use super::{Records, Row, Value, What, attr, reference};
use crate::html_table::is_html;
use crate::role::Role;
use crate::view::Reader;

/// The fewest items an untargeted search takes for a list.
const MIN_ITEMS: usize = 3;
/// The most siblings an item may take along with it.
const MAX_RUN: usize = 4;

/// What makes siblings of a kind.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Kind {
    name: Option<madcawl_str::Atom>,
    role: Role,
    classes: Vec<String>,
}

fn kind_of(r: &Reader, n: NodeId) -> Option<Kind> {
    if r.dom.kind(n) != NodeKind::Element {
        return None;
    }
    let mut classes: Vec<String> = attr(r.dom, n, atom!("class"))
        .unwrap_or_default()
        .split_ascii_whitespace()
        .map(str::to_owned)
        .collect();
    classes.sort();
    Some(Kind {
        name: r.dom.local_name(n),
        role: r.record(n).map_or(Role::None, |rec| rec.role),
        classes,
    })
}

/// The shown element children of `p`.
fn shown_children(r: &Reader, p: NodeId) -> Vec<NodeId> {
    r.dom
        .children(p)
        .filter(|&c| r.dom.kind(c) == NodeKind::Element && r.writes(c).is_some())
        .collect()
}

/// The shown children of `n`'s parent of `n`'s kind.
fn same_kind(r: &Reader, n: NodeId) -> Vec<NodeId> {
    let (Some(p), Some(k)) = (r.dom.parent(n), kind_of(r, n)) else {
        return vec![n];
    };
    shown_children(r, p)
        .into_iter()
        .filter(|&c| kind_of(r, c).as_ref() == Some(&k))
        .collect()
}

fn seeded(r: &Reader, seed: NodeId) -> Vec<NodeId> {
    let mut best: Vec<NodeId> = vec![seed];
    let mut at = Some(seed);
    while let Some(n) = at.filter(|&n| n != r.dom.document()) {
        let group = same_kind(r, n);
        if group.len() > best.len() {
            best = group;
        }
        at = r.dom.parent(n);
    }
    best
}

/// Inside navigation, a banner or a footer.
fn in_chrome(r: &Reader, n: NodeId) -> bool {
    let mut up = Some(n);
    while let Some(p) = up {
        let role = r.record(p).map_or(Role::None, |rec| rec.role);
        if matches!(role, Role::Navigation | Role::Banner | Role::ContentInfo) {
            return true;
        }
        up = r.dom.parent(p);
    }
    false
}

fn text_len(r: &Reader, n: NodeId) -> usize {
    super::shown_text(r, n).chars().count()
}

/// The group of siblings of a kind with the most text in scope.
fn prominent(r: &Reader) -> Vec<NodeId> {
    let mut best: (usize, Vec<NodeId>) = (0, Vec::new());
    let root = r.root();
    for p in std::iter::once(root).chain(r.dom.descendants(root)) {
        if r.dom.kind(p) != NodeKind::Element || r.writes(p).is_none() || in_chrome(r, p) {
            continue;
        }
        let mut groups: HashMap<Kind, Vec<NodeId>> = HashMap::new();
        for c in shown_children(r, p) {
            if let Some(k) = kind_of(r, c) {
                groups.entry(k).or_default().push(c);
            }
        }
        for g in groups.into_values().filter(|g| g.len() >= MIN_ITEMS) {
            // by the median, so one item holding the whole page does not
            // make its few siblings a list
            let mut lens: Vec<usize> = g.iter().map(|&n| text_len(r, n)).collect();
            lens.sort_unstable();
            let score = lens[lens.len() / 2] * g.len();
            if score > best.0 {
                best = (score, g);
            }
        }
    }
    best.1
}

/// Each item's nodes. When every gap between two items holds the same
/// run of kinds, the run belongs to an item: to the item before it, or,
/// when the same run also comes just before the first item (a `dt`
/// before its `dd`), to the item after it.
fn records(r: &Reader, items: &[NodeId]) -> Vec<Vec<NodeId>> {
    let single = || items.iter().map(|&i| vec![i]).collect();
    let Some(parent) = items.first().and_then(|&i| r.dom.parent(i)) else {
        return single();
    };
    let sibs = shown_children(r, parent);
    let index: HashMap<NodeId, usize> = sibs.iter().enumerate().map(|(i, &s)| (s, i)).collect();
    let at: Vec<usize> = items.iter().filter_map(|i| index.get(i).copied()).collect();
    if at.len() != items.len() || at.len() < 2 {
        return single();
    }
    let kinds = |v: &[NodeId]| v.iter().map(|&c| kind_of(r, c)).collect::<Vec<_>>();
    let gap = |i: usize| &sibs[at[i] + 1..at[i + 1]];
    let pattern = kinds(gap(0));
    let k = pattern.len();
    let regular = k > 0 && k <= MAX_RUN && (1..at.len() - 1).all(|i| kinds(gap(i)) == pattern);
    if !regular {
        return single();
    }
    let leading = at[0] >= k && kinds(&sibs[at[0] - k..at[0]]) == pattern;
    let mut out = Vec::with_capacity(at.len());
    for (i, &a) in at.iter().enumerate() {
        let run = match leading {
            true => &sibs[a - k..=a],
            false => &sibs[a..(a + 1 + k).min(sibs.len())],
        };
        // the last item takes a trailing run only when it is the same run
        let whole = leading || i + 1 < at.len() || kinds(&run[1..]) == pattern;
        out.push(if whole { run.to_vec() } else { vec![sibs[a]] });
    }
    out
}

/// The blocks of text in `n`: the text of each run of inline content,
/// collapsed.
fn parts(r: &Reader, n: NodeId, out: &mut Vec<String>) {
    let mut cur = String::new();
    let block_of = |d: NodeId| {
        let mut up = r.dom.parent(d);
        while let Some(p) = up.filter(|&p| p != n) {
            if !r
                .dom
                .local_name(p)
                .is_some_and(|a| super::INLINE.contains(&a))
            {
                return p;
            }
            up = r.dom.parent(p);
        }
        n
    };
    let mut at: Option<NodeId> = None;
    for d in r.dom.descendants(n) {
        if r.dom.kind(d) != NodeKind::Text || r.writes(d).is_none() {
            continue;
        }
        let b = block_of(d);
        if at.is_some_and(|a| a != b) {
            flush(&mut cur, out);
        }
        at = Some(b);
        if let Some(t) = r.dom.text(d) {
            crate::text::push_collapsed(&mut cur, &crate::text::string(t));
        }
    }
    flush(&mut cur, out);
}

fn flush(cur: &mut String, out: &mut Vec<String>) {
    let t = cur.trim();
    if !t.is_empty() {
        out.push(t.to_owned());
    }
    cur.clear();
}

/// The first heading, else the first named link, of the nodes.
fn title(r: &Reader, nodes: &[NodeId]) -> Option<(String, Option<String>)> {
    let all = || {
        nodes
            .iter()
            .flat_map(|&n| std::iter::once(n).chain(r.dom.descendants(n)))
    };
    let named = |role: Role| {
        all().find_map(|d| {
            let rec = r.writes(d).filter(|rec| rec.role == role)?;
            let name = r.name(&rec);
            (!name.is_empty()).then(|| (name.to_owned(), d))
        })
    };
    if let Some((h, _)) = named(Role::Heading) {
        let url = all().find(|&d| is_html(r.dom, d, atom!("a")) && r.writes(d).is_some());
        return Some((h, url.and_then(|a| attr(r.dom, a, atom!("href")))));
    }
    let (t, link) = named(Role::Link)?;
    Some((t, attr(r.dom, link, atom!("href"))))
}

pub(super) fn items(r: &Reader, seed: Option<NodeId>) -> Records {
    let mut recs = Records::new(What::List);
    let items = match seed {
        Some(s) => seeded(r, s),
        None => prominent(r),
    };
    let groups = records(r, &items);
    if let Some(p) = items.first().and_then(|&i| r.dom.parent(i)) {
        recs.source = Some(reference(r, p));
    }
    recs.label = match seed {
        Some(_) => "items of the same kind as the one given".into(),
        None => "the repeated items with the most text".into(),
    };
    recs.columns = ["title", "url", "text", "parts"].map(String::from).to_vec();
    for nodes in groups {
        let item = nodes[0];
        let mut ps = Vec::new();
        for &n in &nodes {
            parts(r, n, &mut ps);
        }
        let (mut t, url) =
            title(r, &nodes).unwrap_or_else(|| (ps.first().cloned().unwrap_or_default(), None));
        // a link around the whole item names all of it: its first block is the title
        if ps.len() > 1 && t == ps.join(" ") {
            t = ps[0].clone();
        }
        let fields = vec![
            ("title".into(), Value::Text(t)),
            ("url".into(), Value::Text(url.unwrap_or_default())),
            ("text".into(), Value::Text(ps.join(" "))),
            ("parts".into(), Value::List(ps)),
        ];
        recs.rows.push(Row {
            node: item,
            reference: reference(r, item),
            fields,
        });
    }
    recs
}
