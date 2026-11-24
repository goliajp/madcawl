//! The elements a restyle starts from, and what appending an element
//! reaches.

use std::collections::{HashMap, HashSet};

use madcawl_dom::{Dom, NodeId, NodeKind};

use super::features::{Features, Tier};
use super::restyle::Mode;

/// The elements to restyle, as roots: each with what below it to
/// restyle; the elements whose later element siblings are to be
/// restyled, expanded once per parent from the earliest of them; and
/// those whose earlier element siblings are, expanded once per parent
/// up to the latest of them.
#[derive(Default)]
pub(super) struct Roots {
    pub list: Vec<(NodeId, Mode)>,
    pub after: Vec<(NodeId, Mode)>,
    pub before: Vec<(NodeId, Mode)>,
    /// Siblings walked to find what to restyle, counted as visits
    pub steps: u64,
}

fn parents(dom: &Dom, starts: &[(NodeId, Mode)]) -> Vec<(NodeId, Mode)> {
    let mut v: Vec<(NodeId, Mode)> = starts
        .iter()
        .filter_map(|&(n, m)| Some((dom.parent(n)?, m)))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

impl Roots {
    pub fn expand(&mut self, dom: &Dom) {
        if !self.after.is_empty() {
            let starts: HashSet<(NodeId, Mode)> = self.after.iter().copied().collect();
            for (p, m) in parents(dom, &self.after) {
                let mut on = false;
                for c in dom.children(p) {
                    self.steps += 1;
                    if on && dom.kind(c) == NodeKind::Element {
                        self.list.push((c, m));
                    }
                    on |= starts.contains(&(c, m));
                }
            }
        }
        if !self.before.is_empty() {
            let starts: HashSet<(NodeId, Mode)> = self.before.iter().copied().collect();
            for (p, m) in parents(dom, &self.before) {
                let mut on = false;
                let mut c = dom.last_child(p);
                while let Some(x) = c {
                    self.steps += 1;
                    if on && dom.kind(x) == NodeKind::Element {
                        self.list.push((x, m));
                    }
                    on |= starts.contains(&(x, m));
                    c = dom.previous_sibling(x);
                }
            }
        }
    }
}

/// Restyle what a change of tier `t` on `el` reaches; true when that is
/// the document.
pub(super) fn reach(dom: &Dom, el: NodeId, t: Tier, roots: &mut Roots) -> bool {
    if t.has(Tier::FULL) {
        return true;
    }
    if t.has(Tier::OWN) {
        roots.list.push((el, Mode::Own));
    }
    if t.has(Tier::SUBTREE) {
        roots.list.push((el, Mode::Subtree));
    }
    if t.has(Tier::SIBLING_TREES) {
        roots.after.push((el, Mode::Subtree));
    } else if t.has(Tier::SIBLINGS) {
        roots.after.push((el, Mode::Own));
    }
    if t.has(Tier::PARENT_SUBTREE) {
        match dom.parent(el).filter(|&p| dom.kind(p) == NodeKind::Element) {
            Some(p) => roots.list.push((p, Mode::Subtree)),
            None => return true,
        }
    }
    false
}

/// How far below an element a positional change to it reaches.
fn below(t: Tier) -> Mode {
    if t.has(Tier::SIBLING_TREES.or(Tier::SUBTREE)) {
        Mode::Subtree
    } else {
        Mode::Own
    }
}

/// The appends of one restyle to one parent, in order.
struct Batch {
    /// The node before the first of them, where looking back at what
    /// was there before starts
    start: Option<NodeId>,
    last: NodeId,
    /// The nearest element before the next append, once looked for
    element: Option<Option<NodeId>>,
    /// Whether the parent had no element or text before the next
    /// append, once looked for
    empty: Option<bool>,
}

/// What the appends of one restyle have found out, so that each looks
/// back past the others at most once.
#[derive(Default)]
pub(super) struct Appends {
    batches: HashMap<NodeId, Batch>,
    /// By parent and element name and namespace, the nearest element of
    /// that name before the next append, once looked for
    same: HashMap<(NodeId, u32, u8), Option<NodeId>>,
}

fn name_of(dom: &Dom, n: NodeId) -> (u32, u8) {
    let name = dom.local_name(n).map_or(u32::MAX, |a| a.index());
    (name, dom.namespace(n).map_or(9, |x| x as u8))
}

/// The nearest node before `from`, itself included, that `want` takes;
/// each node passed is a step.
fn back(
    dom: &Dom,
    from: Option<NodeId>,
    steps: &mut u64,
    mut want: impl FnMut(NodeId) -> bool,
) -> Option<NodeId> {
    let mut c = from;
    while let Some(x) = c {
        *steps += 1;
        if want(x) {
            return Some(x);
        }
        c = dom.previous_sibling(x);
    }
    None
}

fn not_empty(dom: &Dom, x: NodeId) -> bool {
    match dom.kind(x) {
        NodeKind::Element => true,
        NodeKind::Text => dom.text(x).is_some_and(|t| !t.is_empty()),
        _ => false,
    }
}

impl Appends {
    /// Every parent's last append is its last child: the appends of
    /// one restyle are the parents' last children, in order.
    pub fn check(&self, dom: &Dom) {
        for b in self.batches.values() {
            debug_assert!(dom.next_sibling(b.last).is_none(), "appended last");
        }
    }

    /// What appending `n` restyles besides `n` itself: the element
    /// before it was last of its kind, every element before it moved
    /// for `:nth-last-child()`, and the parent was maybe empty.
    pub fn add(&mut self, f: &Features, dom: &Dom, n: NodeId, roots: &mut Roots) -> bool {
        if f.has {
            return true;
        }
        let element = dom.kind(n) == NodeKind::Element;
        if element {
            roots.list.push((n, Mode::Subtree));
        }
        let Some(parent) = dom.parent(n) else {
            return false;
        };
        let b = self.batches.entry(parent).or_insert(Batch {
            start: dom.previous_sibling(n),
            last: n,
            element: None,
            empty: None,
        });
        debug_assert!(
            b.last == n || dom.previous_sibling(n) == Some(b.last),
            "appends in order, each after the one before"
        );
        let mut full = element && self.lasts(f, dom, n, parent, roots);
        let t = f.nth_last;
        if !t.is_none() && element {
            roots.before.push((n, below(t)));
            full |= t.has(Tier::FULL);
        }
        let b = self.batches.get_mut(&parent).expect("just added");
        if !f.empty.is_none() {
            let start = b.start;
            let steps = &mut roots.steps;
            let was_empty = *b
                .empty
                .get_or_insert_with(|| back(dom, start, steps, |x| not_empty(dom, x)).is_none());
            if was_empty && dom.kind(parent) == NodeKind::Element {
                full |= reach(dom, parent, f.empty, roots);
            }
        }
        b.last = n;
        if let Some(e) = &mut b.empty {
            *e &= !not_empty(dom, n);
        }
        full
    }

    /// The element `n` follows, and the one of its name, were last.
    fn lasts(
        &mut self,
        f: &Features,
        dom: &Dom,
        n: NodeId,
        parent: NodeId,
        roots: &mut Roots,
    ) -> bool {
        let b = self.batches.get_mut(&parent).expect("a batch");
        let start = b.start;
        let mut full = false;
        let mut last_one = |p: NodeId, t: Tier, roots: &mut Roots| {
            roots.list.push((p, below(t)));
            // in an `of S` list, the parent's subtree; what reaches
            // siblings after `p` reaches only `n`, styled already
            full |= t.has(Tier::FULL)
                || t.has(Tier::PARENT_SUBTREE) && reach(dom, p, Tier::PARENT_SUBTREE, roots);
        };
        if !f.last_child.is_none() {
            let steps = &mut roots.steps;
            let el = *b.element.get_or_insert_with(|| {
                back(dom, start, steps, |x| dom.kind(x) == NodeKind::Element)
            });
            if let Some(p) = el {
                last_one(p, f.last_child, roots);
            }
        }
        b.element = Some(Some(n));
        if !f.last_of_type.is_none() {
            let name = name_of(dom, n);
            let steps = &mut roots.steps;
            let key = (parent, name.0, name.1);
            let same = *self.same.entry(key).or_insert_with(|| {
                back(dom, start, steps, |x| {
                    dom.kind(x) == NodeKind::Element && name_of(dom, x) == name
                })
            });
            if let Some(p) = same {
                last_one(p, f.last_of_type, roots);
            }
            self.same.insert(key, Some(n));
        }
        full
    }
}

pub(super) fn depth(dom: &Dom, n: NodeId) -> usize {
    std::iter::successors(dom.parent(n), |&p| dom.parent(p)).count()
}
