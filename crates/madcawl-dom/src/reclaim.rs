//! Freeing nodes that have left every tree.
//!
//! While a parser runs it holds handles to nodes that no count records
//! (its open elements, its formatting list, its form pointer), so the
//! DOM frees nothing then: a node taken out of the tree stays allocated.
//! `reclaim` frees them once nothing holds such handles.

use crate::dom::{Dom, Rare};
use crate::node::NodeKind;

impl Dom {
    /// Which slots hold a node that is reachable: in a document's tree,
    /// in the contents of a reachable template, or in a tree a kept-alive
    /// wrapper holds.
    pub(crate) fn reachable(&self) -> Vec<bool> {
        let high = self.arena.high() as usize;
        let mut seen = vec![false; high];
        let mut work: Vec<u32> = (1..high as u32)
            .filter(|&i| {
                self.arena.is_live(i)
                    && self.n(i).parent == 0
                    && (self.kind_of(i) == NodeKind::Document || self.has_kept_wrapper(i))
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

    /// Some node in the tree rooted at `root` has a kept-alive wrapper.
    fn has_kept_wrapper(&self, root: u32) -> bool {
        let mut x = root;
        while x != 0 {
            if self.n(x).wrapper & 1 != 0 {
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
    /// a node outside every tree is no longer valid afterwards.
    pub fn reclaim(&mut self) -> u32 {
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
            freed += 1;
        }
        for r in self.rare.iter_mut() {
            if r.form != 0 && !seen[r.form as usize] {
                r.form = 0;
            }
        }
        freed
    }
}
