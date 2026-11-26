//! Counted node handles: how the host keeps a node across calls (an
//! event's path, a range's boundary points, a mutation record).

use std::marker::PhantomData;

use crate::dom::{Dom, NodeId};

/// A node held by a count of its own, as a wrapper holds its node. The
/// cycle collector finds a count no edge it was told of explains, so
/// the node, and every node it reaches, stays out of the garbage set.
/// Taken with [`Dom::hold`] and given back with [`Dom::let_go`]; not
/// `Clone`, so each count is given back once.
///
/// Dropping one instead of giving it back leaks its count: the node
/// and what it reaches are never freed. A debug build panics on such a
/// drop. Not `Send`: counts change only on the DOM's thread.
///
/// ```compile_fail
/// fn send<T: Send>(_: T) {}
/// let mut dom = madcawl_dom::Dom::new();
/// let doc = dom.document();
/// send(dom.hold(doc));
/// ```
#[must_use = "a held node carries a count: give it back with Dom::let_go"]
#[derive(Debug, PartialEq, Eq)]
pub struct HeldNode(NodeId, PhantomData<*const ()>);

impl Drop for HeldNode {
    fn drop(&mut self) {
        // not while unwinding: a second panic would abort
        debug_assert!(std::thread::panicking(), "a held node dropped without Dom::let_go: its count leaks");
    }
}

impl HeldNode {
    pub fn id(&self) -> NodeId {
        self.0
    }
}

impl Dom {
    /// Hold `n` until [`Dom::let_go`]; `n` is live, and from now on the
    /// DOM frees by count.
    pub fn hold(&mut self, n: NodeId) -> HeldNode {
        let i = n.index();
        self.by_count = true;
        self.inc(i);
        *self.held.entry(i).or_insert(0) += 1;
        HeldNode(n, PhantomData)
    }

    /// Give back the count of `h`. The node may be freed at the next
    /// [`Dom::free_dead`], or become a suspect when it is outside every
    /// document and still counted.
    pub fn let_go(&mut self, h: HeldNode) {
        let i = h.0.index();
        std::mem::forget(h);
        let k = self.held.get_mut(&i).expect("a node this DOM holds");
        *k -= 1;
        if *k == 0 {
            self.held.remove(&i);
        }
        self.dec(i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_node_outlives_its_tree_and_is_freed_once_let_go() {
        let mut dom = Dom::new();
        let doc = dom.document();
        let p = dom.new_element(b"div").unwrap();
        let c = dom.new_element(b"i").unwrap();
        dom.insert_node(doc, p, None);
        dom.insert_node(p, c, None);
        let (h1, h2) = (dom.hold(c), dom.hold(c));
        assert_eq!(dom.ref_count(c), 3, "its parent and two handles");
        assert_eq!(dom.audit().mismatches(), 0, "the audit counts the handles");
        dom.remove_node(c);
        let mut s = Vec::new();
        dom.take_suspects(&mut s);
        assert_eq!(s, vec![c], "held outside every document");
        let mut freed = Vec::new();
        dom.free_dead(&mut freed);
        assert!(freed.is_empty());
        dom.let_go(h1);
        dom.free_dead(&mut freed);
        assert!(freed.is_empty(), "one handle still holds it");
        assert_eq!(dom.audit().mismatches(), 0);
        dom.let_go(h2);
        dom.free_dead(&mut freed);
        assert_eq!(freed, vec![c]);
        assert_eq!(dom.audit().mismatches(), 0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "a held node dropped without Dom::let_go")]
    fn a_held_node_dropped_without_being_given_back_panics_in_a_debug_build() {
        let mut dom = Dom::new();
        let el = dom.new_element(b"div").unwrap();
        drop(dom.hold(el));
    }

    #[test]
    #[should_panic(expected = "a DOM freed by count")]
    fn reclaim_is_refused_once_the_dom_frees_by_count() {
        let mut dom = Dom::new();
        let el = dom.new_element(b"div").unwrap();
        let h = dom.hold(el);
        dom.let_go(h);
        dom.reclaim();
    }

    #[test]
    #[should_panic(expected = "a DOM freed by count")]
    fn reclaim_is_refused_once_a_node_has_had_a_wrapper() {
        let mut dom = Dom::new();
        let el = dom.new_element(b"div").unwrap();
        dom.set_wrapper(el, 0x1000);
        dom.clear_wrapper(el);
        dom.reclaim();
    }
}
