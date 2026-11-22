//! Per select, its first option and its first selectedcontent, kept as
//! nodes go in and out below it, so that an option finishing does not
//! scan the select. A change the cache cannot follow (a subtree with
//! children moved in or out) marks it unknown, and the next use scans.

use madcawl_str::{Atom, atom};

use crate::dom::Dom;

impl Dom {
    /// The nearest select at or above `p`.
    fn select_of(&self, mut p: u32) -> u32 {
        while p != 0 && !self.is_html(p, atom!("select")) {
            p = self.n(p).parent;
        }
        p
    }

    fn tracked(&self, c: u32) -> Option<Atom> {
        [atom!("option"), atom!("selectedcontent")]
            .into_iter()
            .find(|&a| self.is_html(c, a))
    }

    fn has_children(&self, c: u32) -> bool {
        self.kind_of(c).is_container() && self.n(c).first() != 0
    }

    /// `c` has just gone into `p`.
    pub(crate) fn select_inserted(&mut self, p: u32, c: u32) {
        let tracked = self.tracked(c);
        let subtree = self.has_children(c);
        if tracked.is_none() && !subtree {
            return;
        }
        let s = self.select_of(p);
        let r = self.n(s).rare as usize;
        if s == 0 || !self.rare[r].select_known {
            return;
        }
        if subtree {
            self.rare[r].select_known = false;
            return;
        }
        let option = tracked == Some(atom!("option"));
        let entry = &self.rare[r];
        let have = if option {
            entry.select_option
        } else {
            entry.select_content
        };
        if have == 0 || self.precedes(c, have) {
            let r = &mut self.rare[r];
            if option {
                r.select_option = c;
            } else {
                r.select_content = c;
            }
        }
    }

    /// `c` is about to leave `p`.
    pub(crate) fn select_removing(&mut self, p: u32, c: u32) {
        if self.tracked(c).is_none() && !self.has_children(c) {
            return;
        }
        let s = self.select_of(p);
        let r = self.n(s).rare as usize;
        if s == 0 || !self.rare[r].select_known {
            return;
        }
        let (o, k) = (self.rare[r].select_option, self.rare[r].select_content);
        if self.is_inclusive_ancestor(c, o) || self.is_inclusive_ancestor(c, k) {
            self.rare[r].select_known = false;
        }
    }

    /// The first option and the first selectedcontent below `s`.
    pub(crate) fn select_firsts(&mut self, s: u32) -> (u32, u32) {
        let r = self.n(s).rare as usize;
        if r != 0 && self.rare[r].select_known {
            return (self.rare[r].select_option, self.rare[r].select_content);
        }
        let find = |d: &Dom, name| d.find_below(s, |d, x| d.is_html(x, name)).unwrap_or(0);
        let (o, k) = (
            find(self, atom!("option")),
            find(self, atom!("selectedcontent")),
        );
        let r = self.rare_mut(s);
        (r.select_option, r.select_content, r.select_known) = (o, k, true);
        (o, k)
    }

    pub(crate) fn find_below(&self, root: u32, f: impl Fn(&Dom, u32) -> bool) -> Option<u32> {
        let mut x = self.next_in(root, root);
        while x != 0 {
            if f(self, x) {
                return Some(x);
            }
            x = self.next_in(x, root);
        }
        None
    }

    pub(crate) fn is_inclusive_ancestor(&self, a: u32, mut x: u32) -> bool {
        while x != 0 {
            if x == a {
                return true;
            }
            x = self.n(x).parent;
        }
        false
    }

    fn depth(&self, mut x: u32) -> usize {
        let mut d = 0;
        while self.n(x).parent != 0 {
            x = self.n(x).parent;
            d += 1;
        }
        d
    }

    /// `a` comes before `b` in tree order (both in one tree).
    fn precedes(&self, mut a: u32, mut b: u32) -> bool {
        let (depth_a, depth_b) = (self.depth(a), self.depth(b));
        let (mut da, mut db) = (depth_a, depth_b);
        while da > db {
            a = self.n(a).parent;
            da -= 1;
        }
        while db > da {
            b = self.n(b).parent;
            db -= 1;
        }
        if a == b {
            // one is an ancestor of the other, and comes first
            return depth_a < depth_b;
        }
        while self.n(a).parent != self.n(b).parent {
            a = self.n(a).parent;
            b = self.n(b).parent;
        }
        let mut x = self.n(a).next;
        while x != 0 {
            if x == b {
                return true;
            }
            x = self.n(x).next;
        }
        false
    }
}
