//! The elements by the value of one attribute (`id`, a label's `for`),
//! kept up to date from the DOM's change record: names follow
//! `aria-labelledby` and `label for` through it.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::rc::Rc;

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_str::Atom;

use crate::text::attr_string;

/// A hash of a node's slot number: the DOM hands those out, so a page
/// cannot pick ones that collide.
#[derive(Default)]
struct SlotHasher(u64);

impl Hasher for SlotHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }

    fn write_u32(&mut self, n: u32) {
        self.write_u64(u64::from(n));
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

/// The elements with one value: most values have one.
enum Nodes {
    One(NodeId),
    Many(Vec<NodeId>),
}

impl Nodes {
    fn as_slice(&self) -> &[NodeId] {
        match self {
            Nodes::One(n) => std::slice::from_ref(n),
            Nodes::Many(v) => v,
        }
    }
}

pub(crate) struct AttrIndex {
    attr: Atom,
    /// Only elements of this local name, when set
    only: Option<Atom>,
    /// The value is one allocation, shared by both maps
    by_value: HashMap<Rc<str>, Nodes>,
    of: HashMap<NodeId, Rc<str>, BuildHasherDefault<SlotHasher>>,
}

impl AttrIndex {
    pub fn new(attr: Atom, only: Option<Atom>) -> Self {
        AttrIndex {
            attr,
            only,
            by_value: HashMap::new(),
            of: HashMap::default(),
        }
    }

    /// Indexes `root` and what is below it; the values it indexed go
    /// into `seen`.
    pub fn add_subtree(&mut self, dom: &Dom, root: NodeId, seen: &mut Vec<Box<str>>) {
        for n in std::iter::once(root).chain(dom.descendants(root)) {
            self.update(dom, n);
            if let Some(v) = self.of.get(&n) {
                seen.push((**v).into());
            }
        }
    }

    /// Drops `root` and what is below it; the values they had go into
    /// `seen`.
    pub fn remove_subtree(&mut self, dom: &Dom, root: NodeId, seen: &mut Vec<Box<str>>) {
        for n in std::iter::once(root).chain(dom.descendants(root)) {
            if let Some(v) = self.of.get(&n) {
                seen.push((**v).into());
            }
            self.remove(n);
        }
    }

    /// Drops `n`, a node that may be freed already.
    pub fn forget(&mut self, n: NodeId) {
        self.remove(n);
    }

    fn remove(&mut self, n: NodeId) {
        let Some(v) = self.of.remove(&n) else {
            return;
        };
        let nodes = self.by_value.get_mut(&v).expect("indexed both ways");
        match nodes {
            Nodes::One(_) => {
                self.by_value.remove(&v);
            }
            Nodes::Many(list) => {
                list.retain(|&x| x != n);
                if let [one] = list[..] {
                    *nodes = Nodes::One(one);
                }
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
        let v: Rc<str> = match self.by_value.get_key_value(v.as_str()) {
            Some((k, _)) => k.clone(),
            None => v.into(),
        };
        match self.by_value.get_mut(&v) {
            Some(Nodes::One(first)) => {
                let first = *first;
                self.by_value.insert(v.clone(), Nodes::Many(vec![first, n]));
            }
            Some(Nodes::Many(list)) => list.push(n),
            None => {
                self.by_value.insert(v.clone(), Nodes::One(n));
            }
        }
        self.of.insert(n, v);
    }

    /// The first element in tree order with the value.
    pub fn first(&self, dom: &Dom, value: &str) -> Option<NodeId> {
        let list = self.by_value.get(value)?.as_slice();
        list.iter().copied().min_by(|&a, &b| tree_order(dom, a, b))
    }

    /// Every element with the value.
    pub fn all(&self, value: &str) -> &[NodeId] {
        self.by_value.get(value).map_or(&[], Nodes::as_slice)
    }

    pub fn value_of(&self, n: NodeId) -> Option<&str> {
        self.of.get(&n).map(|v| &**v)
    }

    pub fn bytes(&self) -> usize {
        let values: usize = self
            .by_value
            .iter()
            .map(|(k, v)| k.len() + 16 + 4 * v.as_slice().len())
            .sum();
        values + self.by_value.capacity() * 40 + self.of.capacity() * 24
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

    /// Elements given ids at random, some sharing one, some changed or
    /// detached; the index agrees with a scan of the tree after each step.
    #[test]
    fn the_index_follows_changes_as_a_scan_of_the_tree() {
        use madcawl_str::atom;

        use super::AttrIndex;
        let mut dom = Dom::new();
        let doc = dom.document();
        let els: Vec<_> = (0..40).map(|_| dom.new_element(b"div").unwrap()).collect();
        let mut index = AttrIndex::new(atom!("id"), None);
        let mut x = 0x9e37_79b9u32;
        let mut next = |n: u32| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x % n
        };
        for step in 0..4000 {
            let e = els[next(40) as usize];
            match next(4) {
                0 if dom.parent(e).is_none() => dom.insert_node(doc, e, None),
                1 if dom.parent(e).is_some() => dom.remove_node(e),
                _ => {
                    let v = ["a", "b", "c", "", "d"][next(5) as usize];
                    dom.set_attribute(e, atom!("id"), v.as_bytes());
                }
            }
            index.update(&dom, e);
            for v in ["a", "b", "c", "d", ""] {
                let mut want: Vec<_> = els
                    .iter()
                    .copied()
                    .filter(|&n| dom.is_connected(n) && !v.is_empty())
                    .filter(|&n| {
                        dom.attribute(n, atom!("id"))
                            .is_some_and(|s| crate::text::string(s) == v)
                    })
                    .collect();
                let mut got = index.all(v).to_vec();
                want.sort_by_key(|n| n.index());
                got.sort_by_key(|n| n.index());
                assert_eq!(got, want, "step {step} value {v:?}");
                assert_eq!(
                    index.first(&dom, v),
                    want.iter().copied().min_by(|&a, &b| tree_order(&dom, a, b))
                );
            }
            for &n in &els {
                let want = dom
                    .is_connected(n)
                    .then(|| dom.attribute(n, atom!("id")).map(crate::text::string))
                    .flatten();
                assert_eq!(
                    index.value_of(n),
                    want.as_deref().filter(|v| !v.is_empty()),
                    "step {step}"
                );
            }
        }
    }

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
