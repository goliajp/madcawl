//! `:nth-child()` and the other structural pseudo-classes that count
//! siblings. An element's position is worked out for all its siblings
//! at once and kept, so a page with many siblings costs one pass over
//! them; only `of S` asks about each sibling for each element.

use std::collections::HashMap;

use madcawl_dom::{NodeId, NodeKind};

use crate::selector::{Nth, NthKind};

use super::matching::{Matcher, Place};

impl Matcher<'_, '_> {
    /// Where `el` is among its siblings, worked out for all of them the
    /// first time one is asked about.
    pub(crate) fn place(&self, el: NodeId) -> Place {
        let i = el.index() as usize;
        if let Some(&p) = self.places.borrow().get(i)
            && p[0] != 0
        {
            return p;
        }
        let first = match self.dom.parent(el) {
            Some(p) => self.dom.first_child(p),
            None => Some(el),
        };
        let siblings = || {
            std::iter::successors(first, |&x| self.dom.next_sibling(x))
                .filter(|&x| self.dom.kind(x) == NodeKind::Element)
        };
        let ty = |x: NodeId| {
            (
                self.dom.local_name(x),
                self.dom.namespace(x).map(|n| n as u8),
            )
        };
        let mut types: HashMap<_, u32> = HashMap::new();
        let mut places = self.places.borrow_mut();
        let mut count = 0;
        for x in siblings() {
            count += 1;
            let t = types.entry(ty(x)).or_default();
            *t += 1;
            let at = x.index() as usize;
            if places.len() <= at {
                places.resize(at + 1, [0; 4]);
            }
            places[at] = [count, 0, *t, 0];
        }
        for x in siblings() {
            let p = &mut places[x.index() as usize];
            p[1] = count + 1 - p[0];
            p[3] = types[&ty(x)] + 1 - p[2];
        }
        places[i]
    }

    pub(crate) fn nth(&self, k: u32, n: &Nth, el: NodeId, parent: u32) -> bool {
        let i = match (&n.of, n.kind) {
            (Some(l), NthKind::Child | NthKind::LastChild) => {
                if !self.list(k, l, el, parent) {
                    return false;
                }
                // `of S` asks about each sibling for each element
                let back = n.kind == NthKind::Child;
                let step = |x: NodeId| {
                    if back {
                        self.prev_element(x)
                    } else {
                        self.next_element(x)
                    }
                };
                1 + std::iter::successors(step(el), |&x| step(x))
                    .filter(|&x| self.list(k, l, x, parent))
                    .count() as i64
            }
            (_, kind) => {
                let p = self.place(el);
                i64::from(match kind {
                    NthKind::Child => p[0],
                    NthKind::LastChild => p[1],
                    NthKind::OfType => p[2],
                    NthKind::LastOfType => p[3],
                })
            }
        };
        let (a, b) = (i64::from(n.a), i64::from(n.b));
        if a == 0 {
            i == b
        } else {
            (i - b) % a == 0 && (i - b) / a >= 0
        }
    }
}
