//! The count audit: walk every node, recompute what its count must be
//! by the crate's rules, and compare; check the links and the
//! in-document bit on the way.

use crate::dom::Dom;
use crate::node::{IN_DOCUMENT, NodeKind};

/// What one audit found. Every mismatch field must be 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Audit {
    pub nodes: u64,
    /// Nodes whose count differs from the edges that point at them.
    pub counts: u64,
    /// Nodes whose parent, sibling, child or document links disagree.
    pub links: u64,
    /// Nodes whose in-document bit is wrong.
    pub flags: u64,
    /// Nodes outside every tree, waiting for `Dom::reclaim`: not a
    /// mismatch, since a parser may still hold them.
    pub detached: u64,
}

impl Audit {
    pub fn mismatches(&self) -> u64 {
        self.counts + self.links + self.flags
    }

    pub fn add(&mut self, other: Audit) {
        self.nodes += other.nodes;
        self.counts += other.counts;
        self.links += other.links;
        self.flags += other.flags;
        self.detached += other.detached;
    }
}

impl Dom {
    pub fn audit(&self) -> Audit {
        let high = self.arena.high() as usize;
        let live = |i: usize| self.arena.is_live(i as u32);
        let mut want = vec![0u32; high];
        let mut claims = vec![0u32; high];
        let mut a = Audit::default();
        for i in (1..high).filter(|&i| live(i)) {
            a.nodes += 1;
            let n = self.n(i as u32);
            let (p, owner) = (n.parent as usize, n.owner as usize);
            if p != 0 {
                want[i] += 1;
                want[p] += 1;
                claims[p] += 1;
            }
            if owner != i {
                want[owner] += 1;
            }
            let r = &self.rare[n.rare as usize];
            for held in [r.contents, r.inert] {
                if held != 0 {
                    want[held as usize] += 1;
                }
            }
            if n.wrapper & 1 != 0 {
                want[i] += 1;
            }
        }
        let roots = self.roots(high);
        let reachable = self.reachable();
        for i in (1..high).filter(|&i| live(i)) {
            let n = self.n(i as u32);
            if n.count() != want[i] {
                a.counts += 1;
            }
            let owner_ok = live(n.owner as usize)
                && self.kind_of(n.owner) == NodeKind::Document
                && n.scope == n.owner;
            // a child belongs to its parent's document (a document is its
            // own)
            let parent_ok = n.parent == 0
                || live(n.parent as usize)
                    && self.kind_of(n.parent).is_container()
                    && self.n(n.parent).owner == n.owner;
            let form = self.rare[n.rare as usize].form;
            let form_ok =
                form == 0 || live(form as usize) && self.kind_of(form) == NodeKind::Element;
            let children_ok =
                !self.kind_of(i as u32).is_container() || self.chain_ok(i as u32, claims[i]);
            if !(owner_ok && parent_ok && children_ok && form_ok) {
                a.links += 1;
            }
            let root = roots[i];
            let inside = root != 0 && self.kind_of(root) == NodeKind::Document;
            if inside != (n.flags & IN_DOCUMENT != 0) {
                a.flags += 1;
            }
            if !reachable[i] {
                a.detached += 1;
            }
        }
        a
    }

    /// The children chain of `p` links back to it both ways, ends at its
    /// last child, and holds every node that names `p` as its parent.
    fn chain_ok(&self, p: u32, claims: u32) -> bool {
        let (mut prev, mut c, mut k) = (0, self.n(p).first(), 0);
        while c != 0 && k <= claims {
            let n = self.n(c);
            if !self.arena.is_live(c) || n.parent != p || n.prev != prev {
                return false;
            }
            (prev, c, k) = (c, n.next, k + 1);
        }
        c == 0 && k == claims && self.n(p).last() == prev
    }

    /// Each node's root, or 0 where the parent links loop.
    fn roots(&self, high: usize) -> Vec<u32> {
        let mut roots = vec![0u32; high];
        let mut path = Vec::new();
        for i in (1..high as u32).filter(|&i| self.arena.is_live(i)) {
            let mut x = i;
            while roots[x as usize] == 0 && path.len() < high {
                path.push(x);
                let p = self.n(x).parent;
                if p == 0 || !self.arena.is_live(p) {
                    roots[x as usize] = x;
                    break;
                }
                x = p;
            }
            let root = if path.len() < high {
                roots[x as usize]
            } else {
                0
            };
            for y in path.drain(..) {
                roots[y as usize] = root;
            }
        }
        roots
    }
}

#[cfg(test)]
mod tests {
    use madcawl_html::{Attrs, Namespace, TreeSink};
    use madcawl_str::atom;

    use super::*;

    #[test]
    fn a_wrong_count_a_broken_link_and_a_wrong_bit_are_each_found() {
        let mut dom = Dom::new();
        let doc = dom.document();
        let html = dom.create_element(Namespace::Html, atom!("html"), Attrs::empty());
        dom.append(doc, html);
        let p = dom.create_element(Namespace::Html, atom!("p"), Attrs::empty());
        dom.append(html, p);
        dom.insert_text(p, None, b"x");
        let clean = dom.audit();
        assert_eq!((clean.nodes, clean.mismatches()), (4, 0));

        let c = dom.n(p.index()).count();
        dom.m(p.index()).set_count(c + 1);
        assert_eq!(dom.audit().counts, 1);
        dom.m(p.index()).set_count(c);

        dom.m(html.index()).set_last(0);
        assert_eq!(dom.audit().links, 1);
        dom.m(html.index()).set_last(p.index());

        dom.m(p.index()).flags &= !IN_DOCUMENT;
        assert_eq!(dom.audit().flags, 1);
        dom.m(p.index()).flags |= IN_DOCUMENT;

        // a node owned by a document other than its parent's
        let other = dom.new_document(true).unwrap();
        let text = dom.n(p.index()).first();
        let doc_i = dom.doc;
        dom.m(text).owner = other;
        dom.m(text).scope = other;
        dom.dec(doc_i);
        dom.inc(other);
        assert_eq!(dom.audit().links, 1);
    }
}
