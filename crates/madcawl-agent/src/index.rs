//! The elements by the value of one attribute (`id`, a label's `for`),
//! kept up to date from the DOM's change record: names follow
//! `aria-labelledby` and `label for` through it.

use std::collections::HashMap;

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_str::Atom;

use crate::text::attr_string;

pub(crate) struct AttrIndex {
    attr: Atom,
    /// Only elements of this local name, when set
    only: Option<Atom>,
    by_value: HashMap<Box<str>, Vec<NodeId>>,
    of: HashMap<NodeId, Box<str>>,
}

impl AttrIndex {
    pub fn new(attr: Atom, only: Option<Atom>) -> Self {
        AttrIndex {
            attr,
            only,
            by_value: HashMap::new(),
            of: HashMap::new(),
        }
    }

    /// Indexes `root` and what is below it; the values it indexed go
    /// into `seen`.
    pub fn add_subtree(&mut self, dom: &Dom, root: NodeId, seen: &mut Vec<Box<str>>) {
        for n in std::iter::once(root).chain(dom.descendants(root)) {
            self.update(dom, n);
            if let Some(v) = self.of.get(&n) {
                seen.push(v.clone());
            }
        }
    }

    /// Drops `root` and what is below it; the values they had go into
    /// `seen`.
    pub fn remove_subtree(&mut self, dom: &Dom, root: NodeId, seen: &mut Vec<Box<str>>) {
        for n in std::iter::once(root).chain(dom.descendants(root)) {
            if let Some(v) = self.of.get(&n) {
                seen.push(v.clone());
            }
            self.remove(n);
        }
    }

    /// Drops `n`, a node that may be freed already.
    pub fn forget(&mut self, n: NodeId) {
        self.remove(n);
    }

    fn remove(&mut self, n: NodeId) {
        if let Some(v) = self.of.remove(&n) {
            let list = self.by_value.get_mut(&v).expect("indexed both ways");
            list.retain(|&x| x != n);
            if list.is_empty() {
                self.by_value.remove(&v);
            }
        }
    }

    /// The element's attribute may have changed.
    pub fn update(&mut self, dom: &Dom, n: NodeId) {
        self.remove(n);
        if dom.kind(n) != NodeKind::Element || !dom.is_connected(n) {
            return;
        }
        if self.only.is_some_and(|o| dom.local_name(n) != Some(o)) {
            return;
        }
        let Some(v) = attr_string(dom, n, self.attr).filter(|v| !v.is_empty()) else {
            return;
        };
        let v: Box<str> = v.into();
        self.by_value.entry(v.clone()).or_default().push(n);
        self.of.insert(n, v);
    }

    /// The first element in tree order with the value.
    pub fn first(&self, dom: &Dom, value: &str) -> Option<NodeId> {
        let list = self.by_value.get(value)?;
        list.iter().copied().min_by(|&a, &b| tree_order(dom, a, b))
    }

    /// Every element with the value.
    pub fn all(&self, value: &str) -> &[NodeId] {
        self.by_value.get(value).map_or(&[], |v| v.as_slice())
    }

    pub fn value_of(&self, n: NodeId) -> Option<&str> {
        self.of.get(&n).map(|v| &**v)
    }

    pub fn bytes(&self) -> usize {
        let keys: usize = self.of.values().map(|v| v.len() * 2 + 48).sum();
        keys + self.by_value.len() * 32
    }
}

/// Orders two connected nodes in tree order: their ancestor chains up
/// to the first common one, then their siblings there.
pub(crate) fn tree_order(dom: &Dom, a: NodeId, b: NodeId) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let depth = |n: NodeId| std::iter::successors(dom.parent(n), |&p| dom.parent(p)).count();
    let (mut x, mut y) = (a, b);
    let (mut dx, mut dy) = (depth(a), depth(b));
    // an ancestor comes before what is below it
    while dx > dy {
        x = dom.parent(x).expect("deeper than its depth");
        dx -= 1;
        if x == y {
            return Ordering::Greater;
        }
    }
    while dy > dx {
        y = dom.parent(y).expect("deeper than its depth");
        dy -= 1;
        if x == y {
            return Ordering::Less;
        }
    }
    if x == y {
        return Ordering::Equal;
    }
    while dom.parent(x) != dom.parent(y) {
        x = dom.parent(x).expect("not yet at the common ancestor");
        y = dom.parent(y).expect("not yet at the common ancestor");
    }
    let mut s = dom.next_sibling(x);
    while let Some(n) = s {
        if n == y {
            return Ordering::Less;
        }
        s = dom.next_sibling(n);
    }
    Ordering::Greater
}

#[cfg(test)]
mod tests {
    use madcawl_dom::Dom;
    use madcawl_str::DomString;

    use super::tree_order;
    use std::cmp::Ordering::*;

    #[test]
    fn tree_order_without_ancestor_chains() {
        let mut dom = Dom::new();
        let doc = dom.document();
        let a = dom.new_element(b"div").unwrap();
        let b = dom.new_element(b"p").unwrap();
        let c = dom.new_element(b"span").unwrap();
        let t = dom.new_text(DomString::from_wtf8(b"x")).unwrap();
        dom.insert_node(doc, a, None);
        dom.insert_node(a, b, None);
        dom.insert_node(a, c, None);
        dom.insert_node(b, t, None);
        assert_eq!(tree_order(&dom, a, b), Less);
        assert_eq!(tree_order(&dom, t, a), Greater);
        assert_eq!(tree_order(&dom, t, c), Less);
        assert_eq!(tree_order(&dom, c, t), Greater);
        assert_eq!(tree_order(&dom, b, b), Equal);
    }
}
