//! What script bindings need from the DOM: nodes made and moved one at
//! a time, the wrapper word, freeing by count, and the counted edges
//! the engine's cycle collector asks for.
//!
//! The wrapper word (offset 48) holds the wrapper's pointer, whose low
//! bit is clear, and in that bit whether the wrapper is preserved. A
//! nonzero word is a wrapper that holds its node, an edge counted on
//! the node. The preserved bit makes the edge the other way, node to
//! wrapper, strong; that count is on the wrapper, which the engine
//! keeps, and the node's bindings report it.

use madcawl_html::Namespace;
use madcawl_str::DomString;

use crate::dom::{Dom, NodeId};
use crate::node::{NodeKind, SUSPECTED};

/// A counted edge out of a node: the counting rules, from the source's
/// side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// A child holds its parent.
    Parent(NodeId),
    /// A parent holds each child.
    Child(NodeId),
    /// A node holds its node document.
    Document(NodeId),
    /// A template holds its contents.
    Contents(NodeId),
    /// A document holds its template contents owner.
    Inert(NodeId),
}

const KEPT: u64 = 1;

impl Dom {
    /// A text node, in no tree; `None` past the node or byte limit.
    pub fn new_text(&mut self, data: DomString) -> Option<NodeId> {
        self.new_data(NodeKind::Text, data)
    }

    pub fn new_comment(&mut self, data: DomString) -> Option<NodeId> {
        self.new_data(NodeKind::Comment, data)
    }

    fn new_data(&mut self, kind: NodeKind, data: DomString) -> Option<NodeId> {
        if !self.take(data.len()) {
            return None;
        }
        let cut = self.cut;
        let n = self.new_char_data(kind, self.doc, data);
        self.cut = cut;
        n.map(NodeId::at)
    }

    /// Put `child` into `parent` before `before`, or last; moved from
    /// where it was. The caller has checked the move is allowed.
    pub fn insert_node(&mut self, parent: NodeId, child: NodeId, before: Option<NodeId>) {
        self.insert(
            parent.index(),
            child.index(),
            before.map_or(0, NodeId::index),
        );
    }

    /// Take `child` out of its parent.
    pub fn remove_node(&mut self, child: NodeId) {
        self.detach(child.index());
    }

    pub fn is_inclusive_ancestor_of(&self, a: NodeId, b: NodeId) -> bool {
        self.is_inclusive_ancestor(a.index(), b.index())
    }

    /// An element in the HTML namespace.
    pub fn in_html_namespace(&self, n: NodeId) -> bool {
        self.namespace(n) == Some(Namespace::Html)
    }

    /// The node document; a document is its own.
    pub fn owner_document(&self, n: NodeId) -> NodeId {
        NodeId::at(self.n(n.index()).owner)
    }

    /// Replace a text, comment or processing instruction node's data.
    pub fn set_text(&mut self, n: NodeId, data: DomString) -> bool {
        let i = n.index();
        assert!(self.kind_of(i).is_char_data(), "character data");
        if !self.take(data.len()) {
            return false;
        }
        self.settle(i);
        let old = std::mem::replace(&mut self.m(i).data, data.into_raw().as_ptr() as u64);
        // SAFETY: the node's count on its old string, given up
        unsafe { crate::text::drop_string(old) };
        self.notify(crate::tree::Change::Text(i));
        true
    }

    /// The wrapper word: the wrapper's pointer, with the preserved bit.
    pub fn wrapper(&self, n: NodeId) -> u64 {
        self.n(n.index()).wrapper
    }

    /// Store a new wrapper's pointer; the wrapper holds the node.
    pub fn set_wrapper(&mut self, n: NodeId, word: u64) {
        let i = n.index();
        assert!(word != 0 && word & KEPT == 0, "a wrapper pointer");
        assert_eq!(self.n(i).wrapper, 0, "one wrapper per node");
        self.by_count = true;
        self.m(i).wrapper = word;
        self.inc(i);
    }

    /// The wrapper is gone: clear the word and its count on the node.
    pub fn clear_wrapper(&mut self, n: NodeId) {
        let i = n.index();
        assert_eq!(
            self.n(i).wrapper & KEPT,
            0,
            "a preserved wrapper does not die"
        );
        assert_ne!(self.n(i).wrapper, 0, "a node with a wrapper");
        self.m(i).wrapper = 0;
        self.dec(i);
    }

    /// Mark the edge from the node to its wrapper strong or weak again.
    pub fn set_wrapper_preserved(&mut self, n: NodeId, preserved: bool) {
        let w = &mut self.m(n.index()).wrapper;
        assert_ne!(*w, 0, "a node with a wrapper");
        *w = if preserved { *w | KEPT } else { *w & !KEPT };
    }

    /// Every counted edge out of `n`, as `traverse` reports them. The
    /// wrapper edges are the bindings': the wrapper's edge to the node
    /// is the engine's to report, and the preserved edge to the wrapper
    /// is a script value.
    pub fn host_edges(&self, n: NodeId, mut f: impl FnMut(Edge)) {
        let i = n.index();
        let node = self.n(i);
        if node.parent != 0 {
            f(Edge::Parent(NodeId::at(node.parent)));
        }
        if self.kind_of(i).is_container() {
            let mut c = node.first();
            while c != 0 {
                f(Edge::Child(NodeId::at(c)));
                c = self.n(c).next;
            }
        }
        if node.owner != i {
            f(Edge::Document(NodeId::at(node.owner)));
        }
        let r = &self.rare[node.rare as usize];
        if r.contents != 0 {
            f(Edge::Contents(NodeId::at(r.contents)));
        }
        if r.inert != 0 {
            f(Edge::Inert(NodeId::at(r.inert)));
        }
    }

    /// The collector found `n` to be garbage: give up its tree edges.
    /// Its document, contents and inert edges go when it is freed. The
    /// node stays readable until then.
    pub fn unlink_node(&mut self, n: NodeId) {
        let i = n.index();
        if self.kind_of(i).is_container() {
            loop {
                let c = self.n(i).first();
                if c == 0 {
                    break;
                }
                self.detach(c);
            }
        }
        self.detach(i);
    }

    /// The nodes whose count dropped and stayed above 0 while outside
    /// every document, since the last call: candidates for the cycle
    /// collector.
    pub fn take_suspects(&mut self, out: &mut Vec<NodeId>) {
        self.by_count = true;
        let mut list = std::mem::take(&mut self.suspects);
        for &i in &list {
            if !self.arena.is_live(i) {
                continue;
            }
            self.m(i).flags &= !SUSPECTED;
            if self.n(i).count() != 0 {
                out.push(NodeId::at(i));
            }
        }
        list.clear();
        self.suspects = list;
    }

    /// Free every node whose count is 0, and what that releases in
    /// turn; each freed node goes into `freed` so that side tables can
    /// drop it. The document of this DOM is never freed.
    ///
    /// Call it when no parser is building into this DOM: a parser holds
    /// nodes no count records.
    pub fn free_dead(&mut self, freed: &mut Vec<NodeId>) {
        let first = freed.len();
        let mut stale = false;
        while let Some(i) = self.zero.pop() {
            if i == self.doc || !self.arena.is_live(i) || self.n(i).count() != 0 {
                continue;
            }
            debug_assert!(self.n(i).parent == 0 && self.n(i).wrapper == 0);
            self.release_contents(i);
            let (owner, rare) = (self.n(i).owner, self.n(i).rare);
            if owner != i {
                self.dec(owner);
            }
            if rare != 0 {
                let r = std::mem::take(&mut self.rare[rare as usize]);
                for held in [r.contents, r.inert] {
                    if held != 0 {
                        self.dec(held);
                    }
                }
                self.rare_free.push(rare);
            }
            stale |= self.n(i).flags & SUSPECTED != 0;
            self.arena.free(i);
            freed.push(NodeId::at(i));
        }
        // the suspects stay live nodes, each once: the list is never
        // longer than the nodes, and a slot used again is not listed twice
        if stale {
            let arena = &self.arena;
            self.suspects.retain(|&s| arena.is_live(s));
        }
        if freed.len() > first {
            for r in self.rare.iter_mut() {
                if r.form != 0 && !self.arena.is_live(r.form) {
                    r.form = 0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use madcawl_str::DomString;

    use super::*;

    #[test]
    fn a_wrapper_holds_its_node_and_the_audit_counts_it() {
        let mut dom = Dom::new();
        let el = dom.new_element(b"div").unwrap();
        assert_eq!(dom.ref_count(el), 0);
        dom.set_wrapper(el, 0x1000);
        assert_eq!(dom.ref_count(el), 1);
        dom.set_wrapper_preserved(el, true);
        assert_eq!(dom.wrapper(el), 0x1001);
        assert_eq!(
            dom.ref_count(el),
            1,
            "the preserved edge counts on the wrapper"
        );
        assert_eq!(dom.audit().mismatches(), 0);
        dom.set_wrapper_preserved(el, false);
        dom.clear_wrapper(el);
        let mut freed = Vec::new();
        dom.free_dead(&mut freed);
        assert_eq!(freed, vec![el]);
        assert_eq!(dom.node_count(), 1);
    }

    #[test]
    fn a_removed_subtree_is_a_suspect_and_its_edges_are_listed() {
        let mut dom = Dom::new();
        let doc = dom.document();
        let a = dom.new_element(b"div").unwrap();
        let b = dom.new_text(DomString::from_wtf8(b"x")).unwrap();
        dom.insert_node(doc, a, None);
        dom.insert_node(a, b, None);
        let mut edges = Vec::new();
        dom.host_edges(a, |e| edges.push(e));
        assert_eq!(
            edges,
            vec![Edge::Parent(doc), Edge::Child(b), Edge::Document(doc)]
        );
        dom.remove_node(a);
        let mut s = Vec::new();
        dom.take_suspects(&mut s);
        assert_eq!(s, vec![a], "held by its child");
        dom.unlink_node(a);
        let mut freed = Vec::new();
        dom.free_dead(&mut freed);
        freed.sort();
        assert_eq!(freed, vec![a, b]);
        assert_eq!(dom.ref_count(doc), 0);
        assert_eq!(dom.audit().mismatches(), 0);
    }

    #[test]
    fn every_decrement_left_outside_a_document_makes_a_suspect_once() {
        let mut dom = Dom::new();
        let doc = dom.document();
        let p = dom.new_element(b"div").unwrap();
        let (a, b) = (
            dom.new_element(b"a").unwrap(),
            dom.new_element(b"b").unwrap(),
        );
        dom.insert_node(p, a, None);
        dom.insert_node(p, b, None);
        let mut s = Vec::new();
        dom.take_suspects(&mut s);
        assert!(s.is_empty(), "nothing dropped yet");
        // a script wrapper holds p as well
        dom.set_wrapper(p, 0x1000);
        // a moves into the document: p, left holding b, is the suspect;
        // then b leaves it too
        dom.insert_node(doc, a, None);
        dom.remove_node(b);
        dom.take_suspects(&mut s);
        assert_eq!(s, vec![p], "once, however often it dropped");
        assert_eq!(dom.ref_count(p), 1);
        dom.take_suspects(&mut s);
        assert_eq!(s.len(), 1, "taken, the list starts again empty");
        dom.clear_wrapper(p);
        dom.take_suspects(&mut s);
        assert_eq!(s.len(), 1, "a node no one counts is no suspect");
    }

    #[test]
    fn freed_suspects_leave_the_list_and_a_reused_slot_is_listed_once() {
        let mut dom = Dom::new();
        let mut freed = Vec::new();
        for _ in 0..100 {
            let p = dom.new_element(b"div").unwrap();
            let (a, b) = (
                dom.new_element(b"a").unwrap(),
                dom.new_element(b"b").unwrap(),
            );
            dom.insert_node(p, a, None);
            dom.insert_node(p, b, None);
            // p, left holding b, becomes a suspect; then the pair is
            // unlinked as the collector would, and freed, with no host
            // ever taking the suspects
            dom.remove_node(a);
            dom.unlink_node(p);
            dom.free_dead(&mut freed);
            assert!(dom.suspects.len() <= dom.node_count() as usize);
            assert_eq!(dom.suspects.len(), 0);
        }
        assert_eq!(dom.node_count(), 1);
        // a node in a slot a freed suspect had
        let p = dom.new_element(b"div").unwrap();
        let a = dom.new_element(b"a").unwrap();
        dom.insert_node(p, a, None);
        dom.set_wrapper(p, 0x1000);
        dom.remove_node(a);
        let mut s = Vec::new();
        dom.take_suspects(&mut s);
        assert_eq!(s, vec![p]);
        dom.clear_wrapper(p);
        dom.free_dead(&mut freed);
    }
}
