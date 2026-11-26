//! Nodes as slots of the arena: their addresses, their counts, and the
//! memory the arena holds.

use crate::arena::Arena;
use crate::dom::{Dom, NodeId};

impl Dom {
    /// The node's reference count.
    pub fn ref_count(&self, n: NodeId) -> u32 {
        self.n(n.index()).count()
    }

    pub fn node_count(&self) -> u32 {
        self.arena.count()
    }

    /// Arena pages with a live node: what the arena holds of memory.
    pub fn used_pages(&self) -> u32 {
        self.arena.used_pages()
    }

    /// The node's address, fixed from creation to release.
    pub fn address(&self, n: NodeId) -> usize {
        self.arena.addr(n.index())
    }

    /// The live node at `addr`, if it is one of this DOM's.
    pub fn node_at(&self, addr: usize) -> Option<NodeId> {
        let ours = Arena::base_of(addr) == self.arena.addr(0) && addr.is_multiple_of(64);
        let i = if ours { self.arena.index(addr) } else { 0 };
        self.arena.is_live(i).then(|| NodeId::at(i))
    }
}
