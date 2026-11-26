//! Collections (`NodeList`, `HTMLCollection`) as side objects. A live
//! one holds its root by a count and finds its nodes again when the
//! DOM changed since it last looked; a fixed one (`querySelectorAll`)
//! holds each of its nodes. `childNodes` and `children` give the same
//! object while one is alive.

use madcawl_dom::{Dom, NodeId, NodeKind};
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
    /// A live collection's nodes when the DOM was at `version`.
    nodes: Vec<NodeId>,
    version: u64,
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

    fn refresh(&mut self, dom: &Dom) {
        let (Some(root), Some(filter)) = (&self.root, &self.filter) else { return };
        if self.version == dom.version() {
            return;
        }
        let root = root.target();
        let Target::Node(root) = root else { unreachable!("a collection's root is a node") };
        self.nodes.clear();
        let element = |&n: &NodeId| dom.kind(n) == NodeKind::Element;
        match filter {
            Filter::Children => self.nodes.extend(dom.children(root)),
            Filter::ElementChildren => self.nodes.extend(dom.children(root).filter(element)),
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
            Filter::Classes(names) => {
                // no element has a class no atom spells
                let want: Option<Vec<Atom>> = names.iter().map(|b| Atom::lookup(b)).collect();
                if let Some(want) = want.filter(|w| !w.is_empty()) {
                    let has = |n: &NodeId| want.iter().all(|a| dom.class_list(*n).1.contains(a));
                    self.nodes.extend(dom.descendants(root).filter(element).filter(has));
                }
            }
        }
        self.version = dom.version();
    }

    fn get(&self, i: usize) -> Option<NodeId> {
        if self.root.is_some() { self.nodes.get(i).copied() } else { self.items.get(i).map(held_node) }
    }

    fn len(&self) -> usize {
        if self.root.is_some() { self.nodes.len() } else { self.items.len() }
    }
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
        let l = ListState { root, filter: Some(filter), items: Vec::new(), nodes: Vec::new(), version: u64::MAX };
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
        let l = ListState { root: None, filter: None, items, nodes: Vec::new(), version: 0 };
        self.sides.add(SideData::List(l))
    }

    /// Collection `r`, its nodes found again if the DOM changed.
    fn fresh_list(&mut self, r: SideRef) -> &ListState {
        let dom = &self.dom;
        let SideData::List(l) = &mut self.sides.get_mut(r).data else { unreachable!("a collection's side object") };
        l.refresh(dom);
        l
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
