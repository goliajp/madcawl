//! Freeing nodes that have left every tree.
//!
//! While a parser runs it holds handles to nodes that no count records
//! (its open elements, its formatting list, its form pointer), so the
//! DOM frees nothing then: a node taken out of the tree stays allocated.
//! `reclaim` frees them once nothing holds such handles.

use crate::dom::Dom;
use crate::tree::Rare;
use crate::node::NodeKind;

impl Dom {
    /// Which slots hold a node that is reachable: in a document's tree,
    /// in the contents of a reachable template, or in a tree a wrapper
    /// holds.
    pub(crate) fn reachable(&self) -> Vec<bool> {
        let high = self.arena.high() as usize;
        let mut seen = vec![false; high];
        let mut work: Vec<u32> = (1..high as u32)
            .filter(|&i| {
                self.arena.is_live(i)
                    && self.n(i).parent == 0
                    && (self.kind_of(i) == NodeKind::Document || self.has_wrapper(i))
            })
            .collect();
        while let Some(root) = work.pop() {
            let mut x = root;
            while x != 0 {
                seen[x as usize] = true;
                let contents = self.rare[self.n(x).rare as usize].contents;
                if contents != 0 && !seen[contents as usize] {
                    work.push(contents);
                }
                x = self.next_in(x, root);
            }
        }
        seen
    }

    /// Some node in the tree rooted at `root` has a wrapper.
    fn has_wrapper(&self, root: u32) -> bool {
        let mut x = root;
        while x != 0 {
            if self.n(x).wrapper != 0 {
                return true;
            }
            x = self.next_in(x, root);
        }
        false
    }

    /// Free every node that is not reachable, and clear form owners
    /// that pointed at them; how many were freed.
    ///
    /// Call it when no parser is building into this DOM: a `NodeId` of
    /// a node outside every tree is no longer valid afterwards. Only for
    /// a DOM nothing holds by count yet: it ignores counts and tells no
    /// engine, so once a node has had a wrapper or a `HeldNode`, or
    /// suspects were taken, nodes go by `free_dead` and the collector.
    pub fn reclaim(&mut self) -> u32 {
        assert!(!self.by_count, "reclaim on a DOM freed by count");
        let seen = self.reachable();
        let mut freed = 0;
        for i in 1..self.arena.high() {
            if !self.arena.is_live(i) || seen[i as usize] {
                continue;
            }
            self.release_contents(i);
            let n = self.n(i);
            let (owner, rare) = (n.owner, n.rare);
            if owner != i {
                self.dec(owner);
            }
            if rare != 0 {
                self.rare[rare as usize] = Rare::default();
                self.rare_free.push(rare);
            }
            self.arena.free(i);
            self.notify(crate::tree::Change::Freed(i));
            freed += 1;
        }
        for r in self.rare.iter_mut() {
            if r.form != 0 && !seen[r.form as usize] {
                r.form = 0;
            }
        }
        self.zero.clear();
        for i in std::mem::take(&mut self.suspects) {
            if self.arena.is_live(i) {
                self.m(i).flags &= !crate::node::SUSPECTED;
            }
        }
        freed
    }

    /// Every live node that has a wrapper, in or out of a tree: what a
    /// host closing its realm lets go of.
    pub fn wrapped(&self) -> Vec<crate::NodeId> {
        (1..self.arena.high())
            .filter(|&i| self.arena.is_live(i) && self.n(i).wrapper != 0)
            .map(crate::NodeId::at)
            .collect()
    }
}
