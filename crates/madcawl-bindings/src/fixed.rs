//! The nodes of a fixed collection (`querySelectorAll`) that were in the
//! document when its wrapper was made: the wrapper has no edge to them,
//! since the document's wrappers live with the realm and making one for
//! each node found would keep them all. When such a node leaves the
//! document, its wrapper becomes a member of the collection's wrapper,
//! once per collection and node, so script that keeps the collection
//! keeps the tree the node is in now, with what script put there.
//!
//! The node's wrapper is taken as it leaves, while the document still
//! keeps it, and held by a count until the host settles; the members
//! are added then, after one look per collection at whether its wrapper
//! lives. A loop removing every node of a collection so costs a count
//! per node, not a call into the engine.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use madcawl_dom::NodeId;
use madcawl_js::{Engine, Member, Owned};

use crate::host::DomHost;
use crate::side::{SideData, SideRef};

pub(crate) struct FixedLists<E: Engine> {
    /// By node index: the fixed collections with a live wrapper that hold
    /// the node and have no edge to its wrapper yet.
    waiting: HashMap<u32, Vec<SideRef>>,
    /// The wrappers of nodes that left, by collection, until the host
    /// settles.
    left: Vec<(SideRef, Owned<E::Value>)>,
    /// The members each collection's wrapper got for nodes that left.
    edges: HashMap<SideRef, Vec<Member<E>>>,
}

impl<E: Engine> Default for FixedLists<E> {
    fn default() -> Self {
        FixedLists { waiting: HashMap::new(), left: Vec::new(), edges: HashMap::new() }
    }
}

impl<E: Engine> DomHost<E> {
    /// The wrapper of fixed collection `r` was just made, with no edge to
    /// `nodes`, which are in the document.
    pub(crate) fn wait_for_removal(&mut self, r: SideRef, nodes: &[NodeId]) {
        for n in nodes {
            self.fixed.waiting.entry(n.index()).or_default().push(r);
        }
    }

    /// The subtree at `root` left the document and is not placed in its
    /// new tree yet: take the wrappers of its nodes collections wait for.
    pub(crate) fn fixed_nodes_left(&mut self, e: &mut E, root: NodeId) {
        if self.fixed.waiting.is_empty() {
            return;
        }
        let left: Vec<(NodeId, Vec<SideRef>)> = std::iter::once(root)
            .chain(self.dom.descendants(root))
            .filter_map(|n| self.fixed.waiting.remove(&n.index()).map(|lists| (n, lists)))
            .collect();
        for (n, lists) in left {
            for r in lists {
                let w = self.wrap(e, n);
                self.fixed.left.push((r, w));
            }
        }
    }

    /// Make the wrappers taken since the last settle members of their
    /// collections' wrappers, those still alive.
    pub(crate) fn settle_fixed(&mut self, e: &mut E) {
        if self.fixed.left.is_empty() {
            return;
        }
        let mut lists: HashMap<SideRef, Option<Owned<E::Value>>> = HashMap::new();
        for (r, node) in std::mem::take(&mut self.fixed.left) {
            if let Entry::Vacant(v) = lists.entry(r) {
                v.insert(self.live_side_wrapper(e, r));
            }
            if let Some(w) = &lists[&r] {
                let m = Member::of(e, w.get(), node.get());
                self.fixed.edges.entry(r).or_default().push(m);
            }
            e.release(node);
        }
        for w in lists.into_values().flatten() {
            e.release(w);
        }
    }

    /// The wrapper of side object `r` is gone, and with it its members:
    /// when it is a fixed collection, it waits for none of its nodes any
    /// more.
    pub(crate) fn forget_fixed(&mut self, r: SideRef) {
        let SideData::List(l) = &self.sides.get(r).data else { return };
        if l.root.is_some() || self.fixed.waiting.is_empty() && self.fixed.edges.is_empty() {
            return;
        }
        for mut m in self.fixed.edges.remove(&r).unwrap_or_default() {
            m.forget();
        }
        for n in self.list_nodes(r) {
            if let Some(lists) = self.fixed.waiting.get_mut(&n.index()) {
                lists.retain(|&l| l != r);
                if lists.is_empty() {
                    self.fixed.waiting.remove(&n.index());
                }
            }
        }
    }

    pub(crate) fn fixed_edges(&self, r: SideRef) -> impl Iterator<Item = &Member<E>> {
        self.fixed.edges.get(&r).into_iter().flatten()
    }

    /// The host is closing: give back every count and member.
    pub(crate) fn close_fixed(&mut self, e: &mut E) {
        self.fixed.waiting.clear();
        for (_, w) in std::mem::take(&mut self.fixed.left) {
            e.release(w);
        }
        for mut m in std::mem::take(&mut self.fixed.edges).into_values().flatten() {
            m.close(e);
        }
    }
}
