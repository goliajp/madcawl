//! The list of active formatting elements.

use super::tag::{Name, Tag};
use super::{Active, Start, TreeBuilder, TreeSink};
use crate::token::Attribute;

/// The most formatting elements the list keeps after its last marker.
const MAX_ACTIVE: usize = 256;

fn same_attrs(a: &[Attribute], b: &[Attribute]) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|x| b.iter().any(|y| y.name == x.name && y.value == x.value))
}

fn copy_attrs(to: &mut Vec<Attribute>, from: &[Attribute]) {
    to.truncate(from.len());
    let n = to.len();
    for (d, s) in to.iter_mut().zip(from) {
        d.name.clone_from(&s.name);
        d.atom = s.atom;
        d.value.clone_from(&s.value);
    }
    to.extend_from_slice(&from[n..]);
}

impl<S: TreeSink> TreeBuilder<S> {
    /// Push onto the list of active formatting elements, with the
    /// Noah's Ark clause.
    ///
    /// The list after the last marker holds at most `MAX_ACTIVE`
    /// elements: when it is full, the earliest one leaves it, as an
    /// element the Noah's Ark clause drops does. The standard has no
    /// limit, and every new entry is compared with the ones before it,
    /// so without one a page of distinct `<b x=…>` tags costs the
    /// square of its length.
    pub(crate) fn push_active(&mut self, node: S::Handle, tag: Tag, attrs: &[Attribute]) {
        let mut count = 0;
        let mut earliest = None;
        let mut since_marker = 0;
        let mut first_after_marker = None;
        for (i, e) in self.active.iter().enumerate().rev() {
            step!(1 + attrs.len());
            match e {
                Active::Marker => break,
                Active::Element {
                    tag: t, attrs: a, ..
                } if *t == tag && same_attrs(a, attrs) => {
                    count += 1;
                    earliest = Some(i);
                    since_marker += 1;
                    first_after_marker = Some(i);
                }
                Active::Element { .. } => {
                    since_marker += 1;
                    first_after_marker = Some(i);
                }
            }
        }
        if count >= 3
            && let Some(i) = earliest
        {
            self.remove_active(i);
        } else if since_marker >= MAX_ACTIVE
            && let Some(i) = first_after_marker
        {
            self.remove_active(i);
        }
        let mut v = self.spare_attrs.pop().unwrap_or_default();
        copy_attrs(&mut v, attrs);
        self.active.push(Active::Element {
            node,
            tag,
            attrs: v,
        });
    }

    pub(crate) fn push_marker(&mut self) {
        self.active.push(Active::Marker);
    }

    pub(crate) fn remove_active(&mut self, i: usize) {
        if let Active::Element { attrs, .. } = self.active.remove(i) {
            self.spare_attrs.push(attrs);
        }
    }

    pub(crate) fn active_index(&self, node: S::Handle) -> Option<usize> {
        step!(self.active.len());
        self.active
            .iter()
            .rposition(|e| matches!(e, Active::Element { node: n, .. } if *n == node))
    }

    pub(crate) fn open_index(&self, node: S::Handle) -> Option<usize> {
        step!(self.open.len());
        self.open.iter().rposition(|e| e.node == node)
    }

    /// The last formatting element named `tag` after the last marker.
    pub(crate) fn active_after_marker(&self, tag: Tag) -> Option<usize> {
        for (i, e) in self.active.iter().enumerate().rev() {
            step!(1);
            match e {
                Active::Marker => return None,
                Active::Element { tag: t, .. } if *t == tag => return Some(i),
                Active::Element { .. } => {}
            }
        }
        None
    }

    fn active_is_open(&self, i: usize) -> bool {
        match &self.active[i] {
            Active::Marker => true,
            Active::Element { node, .. } => self.open_index(*node).is_some(),
        }
    }

    /// Make a new element for the token of formatting entry `i`, as a
    /// child of the current insertion place, and put it in the entry.
    pub(crate) fn recreate_active(
        &mut self,
        i: usize,
        insert: bool,
        parent: S::Handle,
    ) -> S::Handle {
        let Active::Element { tag, attrs, .. } = &mut self.active[i] else {
            unreachable!("a marker has no element");
        };
        let tag = *tag;
        let attrs = std::mem::take(attrs);
        let s = Start {
            name: Name::known(tag),
            attrs: &attrs,
            self_closing: false,
        };
        let el = if insert {
            self.insert_html(&s)
        } else {
            self.create(&s, super::Namespace::Html, parent)
        };
        self.active[i] = Active::Element {
            node: el,
            tag,
            attrs,
        };
        el
    }

    pub(crate) fn reconstruct_active(&mut self) {
        let Some(last) = self.active.len().checked_sub(1) else {
            return;
        };
        if self.active_is_open(last) {
            return;
        }
        let mut i = last;
        while i > 0 && !self.active_is_open(i - 1) {
            i -= 1;
        }
        for j in i..=last {
            let root = self.open[0].node;
            self.recreate_active(j, true, root);
        }
    }

    pub(crate) fn clear_active_to_marker(&mut self) {
        while let Some(e) = self.active.pop() {
            match e {
                Active::Marker => return,
                Active::Element { attrs, .. } => self.spare_attrs.push(attrs),
            }
        }
    }
}
