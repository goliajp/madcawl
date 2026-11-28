//! Which realm a node's wrapper belongs to while child navigables live
//! (`frames.rs`): the one it was made in, that of its node document's
//! window, until a node whose wrapper is a child realm's moves to the
//! document of another window, and its wrapper goes with it. A
//! navigable's realm, detached as it goes, takes along only the wrappers
//! of nodes still in its documents: a node moved to the top-level
//! document keeps its wrapper, and what script put on it, listeners
//! included. A wrapper of the top-level realm stays there wherever its
//! node goes.

use madcawl_dom::NodeId;
use madcawl_js::Engine;

use crate::host::DomHost;

impl<E: Engine> DomHost<E> {
    /// The window whose realm the wrappers of `doc`'s nodes are made in:
    /// the one whose document it is or was, or 0, the top-level one, for
    /// a document of no navigable.
    pub(crate) fn window_for_doc(&self, doc: NodeId) -> u64 {
        let i = doc.index();
        self.frames.by_doc.get(&i).or_else(|| self.frames.former.get(&i)).copied().unwrap_or(0)
    }

    /// The realm a wrapper of `n` is made in (`window_for_doc`).
    pub(crate) fn realm_for(&self, n: NodeId) -> E::Realm {
        if self.frames.by_doc.is_empty() {
            return self.realm;
        }
        match self.window_for_doc(self.dom.owner_document(n)) {
            0 => self.realm,
            w => self.frames.live[&w].realm,
        }
    }

    /// The nodes at and below `r` were inserted or removed: those whose
    /// wrapper is a child realm's and whose document is now another
    /// window's take it there. Before the tree's wrappers are placed.
    pub(crate) fn rehome_moved(&mut self, e: &mut E, r: NodeId) {
        let to = self.window_for_doc(self.dom.owner_document(r));
        let wrapped = &self.frames.wrapped;
        let moved: Vec<NodeId> = std::iter::once(r)
            .chain(self.dom.descendants(r))
            .filter(|n| wrapped.get(&n.index()).is_some_and(|&(_, w)| w != to))
            .collect();
        for n in moved {
            self.rehome(e, n, to);
        }
    }

    /// `n`'s wrapper, of a child realm, belongs to window `to`'s now.
    fn rehome(&mut self, e: &mut E, n: NodeId, to: u64) {
        let (word, _) = self.frames.wrapped[&n.index()];
        e.move_wrapper(word, self.realm_of(to).expect("a live window"));
        if to == 0 {
            self.frames.wrapped.remove(&n.index());
        } else {
            self.frames.wrapped.insert(n.index(), (word, to));
        }
    }

    /// Navigable `w`, whose document is `doc`, goes: the nodes still in
    /// `doc`, or in the initial `about:blank` it replaced, whose wrapper
    /// is its realm's let go of it, as for a wrapper that died, since
    /// once the realm is detached the engine tells no host about its
    /// wrappers. The replaced document's objects are that window's, as
    /// the standard has them, not the parent's. One that moved to another
    /// window's document since the last settle takes its wrapper there
    /// now.
    pub(crate) fn let_go_of_realm_wrappers(&mut self, e: &mut E, w: u64, doc: NodeId) {
        let mine: Vec<(u32, u64)> = self.frames.wrapped.iter().filter(|&(_, &(_, x))| x == w).map(|(&n, &(word, _))| (n, word)).collect();
        for (i, word) in mine {
            let n = NodeId::from_index(i).expect("a node index");
            if !self.dom.is_live(n) || self.dom.wrapper(n) & !1 != word {
                self.frames.wrapped.remove(&i);
                continue;
            }
            let home = self.dom.owner_document(n);
            if home == doc || self.frames.former.get(&home.index()) == Some(&w) {
                self.frames.wrapped.remove(&i);
                self.let_go_of_wrapper(e, n);
            } else {
                self.rehome(e, n, self.window_for_doc(home));
            }
        }
    }
}
