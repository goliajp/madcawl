//! Which nodes moved between trees, for bindings whose script engine
//! keeps the wrappers of one tree together: nothing is recorded until
//! the bindings ask, and they take the record after every change.

use crate::dom::{Dom, NodeId};

impl Dom {
    /// Start recording the node of every insertion and removal.
    pub fn track_moves(&mut self) {
        self.moves.get_or_insert_with(Vec::new);
    }

    /// Move the nodes inserted or removed since the last call into
    /// `out`, oldest first; a node moved twice is there twice, and a
    /// node freed since is left out.
    pub fn take_moves(&mut self, out: &mut Vec<NodeId>) {
        let Some(m) = self.moves.as_mut() else { return };
        out.extend(m.drain(..).filter(|&i| self.arena.is_live(i)).map(NodeId::at));
    }

    /// Whether a node came to be held by a [`crate::HeldNode`], or
    /// stopped being held, since the last call.
    pub fn take_holds_changed(&mut self) -> bool {
        std::mem::take(&mut self.holds_changed)
    }

    /// The nodes [`crate::HeldNode`]s hold, in no order.
    pub fn held_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.held.keys().map(|&i| NodeId::at(i))
    }

    /// Whether `n` names a live node.
    pub fn is_live(&self, n: NodeId) -> bool {
        self.arena.is_live(n.index())
    }

    pub(crate) fn record_move(&mut self, i: u32) {
        if let Some(m) = self.moves.as_mut() {
            m.push(i);
        }
    }

    /// The root of the tree `n` is in: `n` itself when it has no parent.
    pub fn tree_root(&self, n: NodeId) -> NodeId {
        NodeId::at(self.root_of(n.index()))
    }

    /// Whether `n` is in this DOM's document, the tree that lives as
    /// long as the DOM; not in a template's contents or another document.
    pub fn in_main_tree(&self, n: NodeId) -> bool {
        self.is_connected(n) && self.n(n.index()).owner == self.doc
    }
}
