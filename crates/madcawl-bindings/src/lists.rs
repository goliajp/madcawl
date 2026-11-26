//! Collections (`NodeList`, `HTMLCollection`) as side objects. A live
//! one holds its root by a count and finds its nodes again when the
//! DOM changed since it last looked; a fixed one (`querySelectorAll`)
//! holds each of its nodes. `childNodes` and `children` give the same
//! object while one is alive.

use std::collections::HashMap;

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_html::QuirksMode;
use madcawl_js::{ClassDef, Engine, Owned};
use madcawl_str::Atom;

use crate::convert::{IntoJs, Thrown};
use crate::event::{Held, Target};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

/// Which nodes below its root a live collection has.
pub(crate) enum Filter {
    /// `childNodes`.
    Children,
    /// `children`.
    ElementChildren,
    /// `getElementsByTagName`: `*`, or the name as given and in ASCII
    /// lower case for HTML elements.
    Tag { name: Box<[u8]>, lower: Box<[u8]> },
    /// `getElementsByClassName`: the class names.
    Classes(Vec<Box<[u8]>>),
}

pub(crate) struct ListState {
    /// The root of a live collection; `None` for a fixed one.
    pub root: Option<Held>,
    pub filter: Option<Filter>,
    /// A fixed collection's nodes.
    pub items: Vec<Held>,
    /// A live collection's nodes when the DOM was at `stamp`.
    nodes: Vec<NodeId>,
    stamp: Option<(u64, u64)>,
}

impl ListState {
    pub(crate) fn held(&self, f: &mut impl FnMut(&Held)) {
        self.root.iter().chain(&self.items).for_each(f);
    }

    pub(crate) fn release(&mut self, out: &mut Vec<Held>) {
        out.extend(self.root.take().into_iter().chain(std::mem::take(&mut self.items)));
    }

    /// A `NodeList`, not an `HTMLCollection`.
    pub(crate) fn is_node_list(&self) -> bool {
        matches!(self.filter, None | Some(Filter::Children))
    }

    /// What the nodes found last depend on, now: insertions and
    /// removals, and for a collection by class the `class` attributes.
    fn stamp(filter: &Filter, dom: &Dom) -> (u64, u64) {
        let class = if matches!(filter, Filter::Classes(_)) { dom.class_version() } else { 0 };
        (dom.tree_version(), class)
    }

    /// Find a live collection's nodes again if what they depend on
    /// changed; whether it did.
    fn refresh(&mut self, dom: &Dom) -> bool {
        let (Some(root), Some(filter)) = (&self.root, &self.filter) else { return false };
        let stamp = Self::stamp(filter, dom);
        if self.stamp == Some(stamp) {
            return false;
        }
        let Target::Node(root) = root.target() else { unreachable!("a collection's root is a node") };
        self.nodes.clear();
        let element = |&n: &NodeId| dom.kind(n) == NodeKind::Element;
        match filter {
            Filter::Children => self.nodes.extend(dom.children(root)),
            Filter::ElementChildren => self.nodes.extend(dom.children(root).filter(element)),
            // an element in this DOM has no prefix: its qualified name is
            // its local name
            Filter::Tag { name, lower } => {
                let mut local = Vec::new();
                self.nodes.extend(dom.descendants(root).filter(element).filter(|&n| {
                    if &name[..] == b"*" {
                        return true;
                    }
                    local.clear();
                    dom.local_name(n).expect("an element").write_wtf8(&mut local);
                    local[..] == **if dom.in_html_namespace(n) { lower } else { name }
                }));
            }
            Filter::Classes(names) if dom.quirks_mode() == QuirksMode::Quirks => {
                let mut by_list = HashMap::new();
                let mut has = |n: &NodeId| {
                    let (k, list) = dom.class_list(*n);
                    *by_list.entry(k).or_insert_with(|| caseless_all(names, list))
                };
                self.nodes.extend(dom.descendants(root).filter(element).filter(|n| has(n)));
            }
            Filter::Classes(names) => {
                // no element has a class no atom spells
                let want: Option<Vec<Atom>> = names.iter().map(|b| Atom::lookup(b)).collect();
                if let Some(want) = want.filter(|w| !w.is_empty()) {
                    let has = |n: &NodeId| want.iter().all(|a| dom.class_list(*n).1.contains(a));
                    self.nodes.extend(dom.descendants(root).filter(element).filter(has));
                }
            }
        }
        self.stamp = Some(stamp);
        true
    }

    fn get(&self, i: usize) -> Option<NodeId> {
        if self.root.is_some() { self.nodes.get(i).copied() } else { self.items.get(i).map(held_node) }
    }

    fn len(&self) -> usize {
        if self.root.is_some() { self.nodes.len() } else { self.items.len() }
    }
}

/// Every name in `names` is, ASCII case-insensitively, a class in `list`
/// (quirks mode); no names match nothing.
fn caseless_all(names: &[Box<[u8]>], list: &[Atom]) -> bool {
    let mut b = Vec::new();
    !names.is_empty()
        && names.iter().all(|name| {
            list.iter().any(|a| {
                b.clear();
                a.write_wtf8(&mut b);
                b.eq_ignore_ascii_case(name)
            })
        })
}

fn held_node(h: &Held) -> NodeId {
    match h.target() {
        Target::Node(n) => n,
        _ => unreachable!("a collection holds nodes"),
    }
}

/// An index getter's result: no property for `None`.
pub(crate) struct OrAbsent<T>(pub Option<T>);

impl<E: Engine, T: IntoJs<E>> IntoJs<E> for OrAbsent<T> {
    fn into_js(self, e: &mut E, h: &mut DomHost<E>) -> Owned<E::Value> {
        match self.0 {
            Some(x) => x.into_js(e, h),
            // SAFETY: an immediate carries no count
            None => unsafe { Owned::from_raw(E::absent()) },
        }
    }
}

/// A collection's length for its interceptors, which throw nothing:
/// only its wrappers have them.
pub(crate) fn count<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    this: E::Value,
    class: &'static ClassDef<E, DomHost<E>>,
    length: fn(&mut E, &mut DomHost<E>, SideRef) -> Result<u32, Thrown>,
) -> u32 {
    let r = e.host_of(this, class).and_then(|obj| h.sides.of(obj)).expect("a collection's interceptor on its wrapper");
    length(e, h, r).unwrap_or(0)
}

impl<E: Engine> DomHost<E> {
    /// A live collection of the nodes below `root` that `filter` picks.
    pub(crate) fn live_list(&mut self, root: NodeId, filter: Filter) -> SideRef {
        let root = Some(self.hold_target(Target::Node(root)));
        let l = ListState { root, filter: Some(filter), items: Vec::new(), nodes: Vec::new(), stamp: None };
        self.sides.add(SideData::List(l))
    }

    /// `childNodes`, or `children` when `elements`: the same object while
    /// it lives.
    pub(crate) fn child_list(&mut self, root: NodeId, elements: bool) -> SideRef {
        if let Some(&r) = self.same_lists.get(&(root.index(), elements)) {
            return r;
        }
        let r = self.live_list(root, if elements { Filter::ElementChildren } else { Filter::Children });
        self.same_lists.insert((root.index(), elements), r);
        r
    }

    /// A fixed `NodeList` of `nodes`.
    pub(crate) fn fixed_list(&mut self, nodes: Vec<NodeId>) -> SideRef {
        let items = nodes.into_iter().map(|n| self.hold_target(Target::Node(n))).collect();
        let l = ListState { root: None, filter: None, items, nodes: Vec::new(), stamp: None };
        self.sides.add(SideData::List(l))
    }

    /// Collection `r`, its nodes found again if the DOM changed.
    fn fresh_list(&mut self, r: SideRef) -> &ListState {
        let dom = &self.dom;
        let SideData::List(l) = &mut self.sides.get_mut(r).data else { unreachable!("a collection's side object") };
        self.list_rebuilds += u64::from(l.refresh(dom));
        l
    }

    /// How many times live collections found their nodes again since
    /// the host was made.
    pub fn list_rebuilds(&self) -> u64 {
        self.list_rebuilds
    }

    pub(crate) fn list_item(&mut self, r: SideRef, i: u32) -> Option<NodeId> {
        self.fresh_list(r).get(i as usize)
    }

    pub(crate) fn list_len(&mut self, r: SideRef) -> u32 {
        self.fresh_list(r).len() as u32
    }

    /// The nodes of collection `r` now.
    pub(crate) fn list_nodes(&mut self, r: SideRef) -> Vec<NodeId> {
        let l = self.fresh_list(r);
        (0..l.len()).filter_map(|i| l.get(i)).collect()
    }

    /// Side object `r` is being freed: it is no longer the collection
    /// `childNodes` or `children` gives.
    pub(crate) fn forget_list(&mut self, r: SideRef) {
        let SideData::List(l) = &self.sides.get(r).data else { return };
        let key = match (&l.root, &l.filter) {
            (Some(Held::Node(n)), Some(Filter::Children)) => (n.id().index(), false),
            (Some(Held::Node(n)), Some(Filter::ElementChildren)) => (n.id().index(), true),
            _ => return,
        };
        if self.same_lists.get(&key) == Some(&r) {
            self.same_lists.remove(&key);
        }
    }
}
