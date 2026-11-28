//! Tree operations. Each keeps the counts by the crate's rules and the
//! in-document bit, and reports the change in one place (`notify`).

use madcawl_str::DomString;

use crate::dom::Dom;
use crate::node::{IN_DOCUMENT, INERT, Node, NodeKind, STYLE_DIRTY, SUSPECTED};

#[derive(Default)]
pub(crate) struct Rare {
    pub contents: u32,
    pub inert: u32,
    pub form: u32,
    pub doctype: Option<Box<[DomString; 3]>>,
    /// A select's first option and first selectedcontent below it, when
    /// `select_known`; otherwise found again by a scan.
    pub select_option: u32,
    pub select_content: u32,
    pub select_known: bool,
}

/// What changed, as styles and later observers need to know.
pub(crate) enum Change {
    Inserted(u32),
    Removed {
        parent: u32,
        child: u32,
    },
    Attributes(u32),
    Text(u32),
    /// The slot was freed
    Freed(u32),
}

/// The numbers live collections compare before they trust what they found.
#[derive(Default)]
pub(crate) struct Versions {
    /// Counts insertions and removals.
    pub tree: u64,
    /// Counts the changes to `class` attributes.
    pub class: u64,
}

impl Dom {
    pub(crate) fn inc(&mut self, i: u32) {
        let n = self.m(i);
        n.set_count(n.count() + 1);
    }

    /// Every decrement goes through here. One that leaves a node outside
    /// every document still counted may have left it in a cycle only
    /// the collector can free, so it makes the node a suspect: the
    /// child moved out, the parent it left, a template's contents.
    pub(crate) fn dec(&mut self, i: u32) {
        let n = self.m(i);
        n.set_count(n.count() - 1);
        if n.count() == 0 {
            self.zero.push(i);
        } else {
            self.suspect_if_held(i);
        }
    }

    pub(crate) fn suspect_if_held(&mut self, i: u32) {
        let n = self.m(i);
        if n.count() != 0 && n.flags & (IN_DOCUMENT | SUSPECTED) == 0 {
            n.flags |= SUSPECTED;
            self.suspects.push(i);
        }
    }

    /// A number that changes whenever a node is inserted or removed:
    /// what a live collection checks before it trusts the nodes it found
    /// last. Attribute and text changes, and nodes freed, leave it.
    pub fn tree_version(&self) -> u64 {
        self.versions.tree
    }

    /// A number that changes whenever a `class` attribute is set, added
    /// or removed: what a collection by class checks besides
    /// `tree_version`.
    pub fn class_version(&self) -> u64 {
        self.versions.class
    }

    pub(crate) fn notify(&mut self, change: Change) {
        if matches!(change, Change::Inserted(_) | Change::Removed { .. }) {
            self.versions.tree += 1;
        }
        // one predictable branch while nothing observes
        if self.observer.is_some() {
            self.record(&change);
        }
        let node = match change {
            Change::Inserted(n) => {
                self.record_move(n);
                n
            }
            Change::Attributes(n) | Change::Text(n) => n,
            Change::Removed { parent, child } => {
                self.record_move(child);
                parent
            }
            Change::Freed(_) => return,
        };
        self.m(node).flags |= STYLE_DIRTY;
    }

    /// A node in slot of its own, owned by `owner` (which it holds), or
    /// `None` when the DOM holds all the nodes it may.
    pub(crate) fn new_node(&mut self, kind: NodeKind, owner: u32) -> Option<u32> {
        let Some(i) = self.arena.alloc(Node::new(kind, owner), self.limits.nodes) else {
            self.cut = Some(crate::Refused::Nodes);
            return None;
        };
        self.inc(owner);
        Some(i)
    }

    /// A document, its own node document. An inert one is a template
    /// contents owner.
    pub(crate) fn new_document(&mut self, inert: bool) -> Option<u32> {
        let Some(i) = self
            .arena
            .alloc(Node::new(NodeKind::Document, 0), self.limits.nodes)
        else {
            self.cut = Some(crate::Refused::Nodes);
            return None;
        };
        let n = self.m(i);
        n.owner = i;
        n.scope = i;
        n.flags |= IN_DOCUMENT | if inert { INERT } else { 0 };
        Some(i)
    }

    pub(crate) fn rare_mut(&mut self, i: u32) -> &mut Rare {
        if self.n(i).rare == 0 {
            let r = match self.rare_free.pop() {
                Some(r) => r,
                None => {
                    self.rare.push(Rare::default());
                    u32::try_from(self.rare.len() - 1).expect("under 2^32 rare entries")
                }
            };
            self.m(i).rare = r;
        }
        let r = self.n(i).rare as usize;
        &mut self.rare[r]
    }

    /// The document a child of `p` belongs to.
    pub(crate) fn doc_of(&self, p: u32) -> u32 {
        self.n(p).owner
    }

    /// The node after `n` in tree order, staying below `root`; 0 at the
    /// end.
    pub(crate) fn next_in(&self, mut n: u32, root: u32) -> u32 {
        let node = self.n(n);
        if node.kind().is_some_and(NodeKind::is_container) && node.first() != 0 {
            return node.first();
        }
        loop {
            if n == root {
                return 0;
            }
            let node = self.n(n);
            if node.next != 0 {
                return node.next;
            }
            n = node.parent;
        }
    }

    pub(crate) fn root_of(&self, mut n: u32) -> u32 {
        while self.n(n).parent != 0 {
            n = self.n(n).parent;
        }
        n
    }

    /// Take `c` out of its parent, if it has one.
    pub(crate) fn detach(&mut self, c: u32) {
        if self.unlink(c) == 0 {
            return;
        }
        if self.n(c).flags & IN_DOCUMENT != 0 {
            self.set_in_document(c, false);
            // its decrement came while it was still in the document
            self.suspect_if_held(c);
        }
    }

    /// Take `c` out of its parent's child list, leaving its in-document
    /// bits as they are; the parent it had, or 0.
    fn unlink(&mut self, c: u32) -> u32 {
        let Node {
            parent, prev, next, ..
        } = *self.n(c);
        if parent == 0 {
            return 0;
        }
        self.select_removing(parent, c);
        if prev == 0 {
            self.m(parent).set_first(next);
        } else {
            self.m(prev).next = next;
        }
        if next == 0 {
            self.m(parent).set_last(prev);
        } else {
            self.m(next).prev = prev;
        }
        let n = self.m(c);
        (n.parent, n.prev, n.next) = (0, 0, 0);
        self.dec(c);
        self.dec(parent);
        self.notify(Change::Removed { parent, child: c });
        parent
    }

    /// Put `c` into `p` before `before`, or last when it is 0; moved
    /// from where it was.
    pub(crate) fn insert(&mut self, p: u32, c: u32, before: u32) {
        assert!(self.kind_of(p).is_container(), "insert into a container");
        assert!(!self.contains_cheap(c, p), "a node goes into itself or below itself");
        self.unlink(c);
        let prev = if before == 0 {
            self.n(p).last()
        } else {
            self.n(before).prev
        };
        let n = self.m(c);
        (n.parent, n.prev, n.next) = (p, prev, before);
        if prev == 0 {
            self.m(p).set_first(c);
        } else {
            self.m(prev).next = c;
        }
        if before == 0 {
            self.m(p).set_last(c);
        } else {
            self.m(before).prev = c;
        }
        self.inc(c);
        self.inc(p);
        let doc = self.doc_of(p);
        if self.n(c).owner != doc {
            self.adopt(c, doc);
        }
        let inside = self.n(p).flags & IN_DOCUMENT != 0;
        if (self.n(c).flags & IN_DOCUMENT != 0) != inside {
            self.set_in_document(c, inside);
        }
        self.select_inserted(p, c);
        self.notify(Change::Inserted(c));
    }

    /// Make `doc` the node document of `root` and everything below it.
    fn adopt(&mut self, root: u32, doc: u32) {
        let mut x = root;
        while x != 0 {
            let old = self.n(x).owner;
            if old != doc {
                self.dec(old);
                self.inc(doc);
                let n = self.m(x);
                (n.owner, n.scope) = (doc, doc);
            }
            x = self.next_in(x, root);
        }
    }

    fn set_in_document(&mut self, root: u32, on: bool) {
        let mut x = root;
        while x != 0 {
            let n = self.m(x);
            n.flags = if on {
                n.flags | IN_DOCUMENT
            } else {
                n.flags & !IN_DOCUMENT
            };
            x = self.next_in(x, root);
        }
    }
}
