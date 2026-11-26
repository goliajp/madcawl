//! Telling an engine that keeps wrappers by reach which tree each
//! wrapper is in ([`Engine::place_wrappers`]).
//!
//! Every wrapper of a detached tree is placed with the wrapper of the
//! tree's root, which is made when the tree first has a wrapper below
//! it; a wrapper of a node in the document is anchored, and so is every
//! wrapper of a detached tree the host holds a node of (`HeldNode`): an
//! event's target keeps its tree's wrappers as long as the event. The
//! DOM records the nodes each insertion and removal moved and whether
//! the held nodes changed, and every host function and settle places
//! again before script can run, so the engine's collector never sees a
//! tree split across groups.

use madcawl_dom::NodeId;
use madcawl_js::{Engine, Owned, Tree};

use crate::host::DomHost;

impl<E: Engine> DomHost<E> {
    /// Place the wrappers of the subtrees moved since the last call, and
    /// of the trees that came to be held or stopped being held.
    pub(crate) fn regroup(&mut self, e: &mut E) {
        if !E::PLACES_WRAPPERS || self.closed {
            return;
        }
        let mut moved = std::mem::take(&mut self.moved);
        self.dom.take_moves(&mut moved);
        let holds = self.dom.take_holds_changed();
        if moved.is_empty() && !holds && self.unplaced_sides.is_empty() {
            self.moved = moved;
            return;
        }
        let pins = self.pinned_roots();
        let old = std::mem::replace(&mut self.pinned, pins.clone());
        // a tree whose pin changed is placed whole: the root stands for it
        let flipped = old.iter().filter(|r| !pins.contains(r)).chain(pins.iter().filter(|r| !old.contains(r)));
        let flipped: Vec<NodeId> = flipped.copied().filter(|&r| self.dom.is_live(r) && self.dom.tree_root(r) == r).collect();
        let mut again = Vec::new();
        for r in moved.drain(..).chain(flipped) {
            if self.dom.is_live(r) && !self.place_subtree(e, r) {
                again.push(r);
            }
        }
        self.moved = again;
        self.place_sides(e);
    }

    /// The roots of the detached trees a host handle holds a node of,
    /// each once.
    fn pinned_roots(&self) -> Vec<NodeId> {
        let mut roots: Vec<NodeId> =
            self.dom.held_nodes().filter(|&n| !self.dom.in_main_tree(n)).map(|n| self.dom.tree_root(n)).collect();
        roots.sort_unstable_by_key(|n| n.index());
        roots.dedup();
        roots
    }

    fn pinned(&self, root: NodeId) -> bool {
        self.dom.held_nodes().any(|n| !self.dom.in_main_tree(n) && self.dom.tree_root(n) == root)
    }

    /// Place a wrapper just made for `n`.
    pub(crate) fn place_new(&mut self, e: &mut E, n: NodeId, word: u64) {
        if !E::PLACES_WRAPPERS {
            return;
        }
        let root = self.dom.tree_root(n);
        let placed = if self.dom.in_main_tree(n) || self.pinned(root) {
            self.place(e, Tree::Anchored, &[(n, word)])
        } else if root == n {
            self.place(e, Tree::New, &[(n, word)])
        } else {
            let r = self.wrap(e, root);
            let placed = self.place(e, Tree::With(r.get()), &[(n, word)]);
            e.release(r);
            placed
        };
        if !placed {
            self.moved.push(n);
        }
    }

    /// Place every wrapper below `r`, inclusive, in the tree `r` is in;
    /// `false` when the engine could not place them now.
    fn place_subtree(&mut self, e: &mut E, r: NodeId) -> bool {
        let mut words: Vec<(NodeId, u64)> = std::iter::once(r)
            .chain(self.dom.descendants(r))
            .filter_map(|n| Some((n, self.dom.wrapper(n))).filter(|&(_, w)| w != 0))
            .collect();
        if words.is_empty() {
            return true;
        }
        let root = self.dom.tree_root(r);
        if self.dom.in_main_tree(r) || self.pinned(root) {
            return self.place(e, Tree::Anchored, &words);
        }
        if root == r {
            // a tree of its own now, whose root has a live wrapper (made
            // here when it had none or its own had died), which may still
            // be with the tree `r` left
            let w = self.wrap(e, r);
            e.release(w);
            if words[0].0 == r {
                words.remove(0);
            }
            words.insert(0, (r, self.dom.wrapper(r)));
            return self.place(e, Tree::New, &words);
        }
        let root_wrapper = self.wrap(e, root);
        let placed = self.place(e, Tree::With(root_wrapper.get()), &words);
        e.release(root_wrapper);
        placed
    }

    fn place(&mut self, e: &mut E, tree: Tree<E::Value>, words: &[(NodeId, u64)]) -> bool {
        let plain: Vec<u64> = words.iter().map(|&(_, w)| w & !1).collect();
        let mut refused = Vec::new();
        if !e.place_wrappers(tree, &plain, &mut refused) {
            return false;
        }
        for i in refused {
            self.let_go_of_wrapper(e, words[i].0);
        }
        true
    }

    /// The engine refused `n`'s wrapper: let go of it as `wrapper_died`
    /// would.
    pub(crate) fn let_go_of_wrapper(&mut self, e: &mut E, n: NodeId) {
        let w = self.dom.wrapper(n);
        if w & 1 != 0 {
            self.dom.set_wrapper_preserved(n, false);
            // SAFETY: the count the preserved edge held
            e.release(unsafe { Owned::from_raw(E::wrapper_value(w & !1)) });
        }
        self.dom.clear_wrapper(n);
        self.drop_members(n);
    }

    /// `n`'s wrapper is gone: on an engine that keeps members beside the
    /// wrapper, so are its listeners' callbacks.
    pub(crate) fn drop_members(&mut self, n: NodeId) {
        if E::PLACES_WRAPPERS {
            for mut l in self.take_listeners(n) {
                l.callback.forget();
            }
        }
    }
}
