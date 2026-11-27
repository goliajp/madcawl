//! Attributes as `Attr` objects. The DOM keeps an attribute as an entry
//! of its element, not as a node; an `Attr` is a side object made when
//! script asks for one, the same object for an attribute while it lives
//! (found by element, namespace and local name, which no two attributes
//! of one element share). While on an element it holds the element by a
//! count and reads and writes the element's entry; taken off, it keeps
//! the value it had and is on no element. Every path that takes an
//! entry off an element goes through [`DomHost::detach_attr`] first.
//!
//! On an engine that places wrappers, the wrappers of an element's
//! `Attr`s and of its `attributes` map are members of the element's
//! wrapper, so what script put on them lives as long as the element's
//! wrapper does, as an attribute node lives as long as its element; and
//! each holds the element's wrapper (`holds.rs`), so the two die
//! together.

use std::collections::HashMap;

use madcawl_dom::NodeId;
use madcawl_html::AttrNamespace;
use madcawl_js::{Engine, Member};
use madcawl_str::{Atom, DomString};

use crate::event::{Held, Target};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

pub(crate) struct AttrState {
    /// The element, by a count, while the attribute is on it.
    pub owner: Option<Held>,
    pub ns: AttrNamespace,
    /// A static atom: only the parser makes prefixed attributes.
    pub prefix: Option<Atom>,
    pub local: Atom,
    /// The value while it is on no element.
    pub value: DomString,
}

impl AttrState {
    pub(crate) fn element(&self) -> Option<NodeId> {
        self.owner.as_ref().map(|h| match h.target() {
            Target::Node(n) => n,
            _ => unreachable!("an attribute's element is a node"),
        })
    }
}

/// An attribute's key: element index, namespace, local name.
type Key = (u32, u8, u32);

fn key(el: NodeId, ns: AttrNamespace, local: Atom) -> Key {
    (el.index(), ns as u8, local.index())
}

pub(crate) struct Attrs<E: Engine> {
    by_attr: HashMap<Key, SideRef>,
    /// The `attributes` map and the `classList` of each element that has
    /// one, by element index and whether it is the list.
    maps: HashMap<(u32, bool), SideRef>,
    /// The side objects whose wrappers are members of each element's
    /// wrapper, and those members.
    owned: HashMap<u32, Vec<SideRef>>,
    edges: HashMap<SideRef, Member<E>>,
}

impl<E: Engine> Default for Attrs<E> {
    fn default() -> Self {
        Attrs { by_attr: HashMap::new(), maps: HashMap::new(), owned: HashMap::new(), edges: HashMap::new() }
    }
}

impl<E: Engine> DomHost<E> {
    pub(crate) fn attr(&self, r: SideRef) -> &AttrState {
        match &self.sides.get(r).data {
            SideData::Attr(a) => a,
            _ => unreachable!("an attribute's side object"),
        }
    }

    pub(crate) fn attr_mut(&mut self, r: SideRef) -> &mut AttrState {
        match &mut self.sides.get_mut(r).data {
            SideData::Attr(a) => a,
            _ => unreachable!("an attribute's side object"),
        }
    }

    /// The `Attr` of attribute `k` of element `el`, made if it has none.
    pub(crate) fn attr_of(&mut self, el: NodeId, k: usize) -> SideRef {
        let a = self.dom.attributes(el).nth(k).expect("an attribute of the element");
        let (ns, prefix, local) = (a.ns, a.prefix, a.local);
        if let Some(&r) = self.attrs.by_attr.get(&key(el, ns, local)) {
            return r;
        }
        let state = AttrState { owner: None, ns, prefix, local, value: DomString::empty() };
        let r = self.sides.add(SideData::Attr(state));
        self.own_attr(r, el);
        r
    }

    /// A new `Attr` on no element.
    pub(crate) fn loose_attr(&mut self, local: Atom) -> SideRef {
        let state = AttrState { owner: None, ns: AttrNamespace::None, prefix: None, local, value: DomString::empty() };
        self.sides.add(SideData::Attr(state))
    }

    /// The place among `el`'s attributes of the one `Attr` `r` is for.
    pub(crate) fn place_of(&self, el: NodeId, r: SideRef) -> Option<usize> {
        let a = self.attr(r);
        self.dom.attributes(el).position(|x| x.ns == a.ns && x.local == a.local)
    }

    /// Put `r`, on no element, on `el`, whose attribute it now is.
    fn own_attr(&mut self, r: SideRef, el: NodeId) {
        let held = self.hold_target(Target::Node(el));
        let a = self.attr_mut(r);
        a.owner = Some(held);
        a.value = DomString::empty();
        let k = key(el, a.ns, a.local);
        self.attrs.by_attr.insert(k, r);
        self.attrs.owned.entry(el.index()).or_default().push(r);
    }

    /// Attach `r`, a script's `Attr` now in the DOM as an attribute of
    /// `el`, with its wrapper's members.
    pub(crate) fn attach_attr(&mut self, e: &mut E, r: SideRef, el: NodeId) {
        self.own_attr(r, el);
        if let Some(w) = self.live_side_wrapper(e, r) {
            self.keep_owned(e, r, w.get());
            self.keep_target(e, r, w.get());
            e.release(w);
        }
    }

    /// The attribute `k` of `el` is about to be taken off: its `Attr`,
    /// if it has one, keeps its value and is on no element from now.
    pub(crate) fn detach_attr(&mut self, e: &mut E, el: NodeId, k: usize) -> Option<SideRef> {
        let a = self.dom.attributes(el).nth(k).expect("an attribute of the element");
        let (k, value) = (key(el, a.ns, a.local), a.value.clone());
        let r = self.attrs.by_attr.remove(&k)?;
        self.disown(el, r);
        if let Some(mut m) = self.attrs.edges.remove(&r) {
            m.clear(e);
        }
        if let Some(mut m) = self.target_edges.remove(&r) {
            m.clear(e);
        }
        let held = self.attr_mut(r).owner.take().expect("an attribute on its element");
        self.attr_mut(r).value = value;
        self.let_go_held(held);
        Some(r)
    }

    fn disown(&mut self, el: NodeId, r: SideRef) {
        if let Some(list) = self.attrs.owned.get_mut(&el.index()) {
            list.retain(|&x| x != r);
            if list.is_empty() {
                self.attrs.owned.remove(&el.index());
            }
        }
    }

    /// The `attributes` map of `el`: the same object while it lives.
    pub(crate) fn attr_map(&mut self, el: NodeId) -> SideRef {
        self.element_object(el, false)
    }

    /// The `classList` of `el`: the same object while it lives.
    pub(crate) fn token_list(&mut self, el: NodeId) -> SideRef {
        self.element_object(el, true)
    }

    fn element_object(&mut self, el: NodeId, list: bool) -> SideRef {
        if let Some(&r) = self.attrs.maps.get(&(el.index(), list)) {
            return r;
        }
        let held = self.hold_target(Target::Node(el));
        let r = self.sides.add(if list { SideData::TokenList(held) } else { SideData::AttrMap(held) });
        self.attrs.maps.insert((el.index(), list), r);
        self.attrs.owned.entry(el.index()).or_default().push(r);
        r
    }

    /// The element an `Attr`, an `attributes` map or a `classList`
    /// belongs to.
    pub(crate) fn owning_element(&self, r: SideRef) -> Option<NodeId> {
        match &self.sides.get(r).data {
            SideData::Attr(a) => a.element(),
            SideData::AttrMap(Held::Node(n)) | SideData::TokenList(Held::Node(n)) => Some(n.id()),
            _ => None,
        }
    }

    /// On an engine that places wrappers, the wrapper `v` just made for
    /// `r`, an element's `Attr`, `attributes` map or `classList`, is a
    /// member of the element's wrapper.
    pub(crate) fn keep_owned(&mut self, e: &mut E, r: SideRef, v: E::Value) {
        if !E::PLACES_WRAPPERS {
            return;
        }
        let Some(el) = self.owning_element(r) else { return };
        let owner = self.wrap(e, el);
        let m = Member::of(e, owner.get(), v);
        e.release(owner);
        if let Some(mut old) = self.attrs.edges.insert(r, m) {
            old.forget();
        }
    }

    /// Side object `r` is being freed, or its wrapper died: it is no
    /// longer an element's `Attr` or map, and the member its wrapper was
    /// went with that wrapper.
    pub(crate) fn forget_owned(&mut self, r: SideRef, freed: bool) {
        if let Some(mut m) = self.attrs.edges.remove(&r) {
            m.forget();
        }
        if !freed {
            return;
        }
        let (el, k) = match &self.sides.get(r).data {
            SideData::Attr(a) => match a.element() {
                Some(el) => (el, Some(key(el, a.ns, a.local))),
                None => return,
            },
            SideData::AttrMap(Held::Node(n)) | SideData::TokenList(Held::Node(n)) => (n.id(), None),
            _ => return,
        };
        match k {
            Some(k) if self.attrs.by_attr.get(&k) == Some(&r) => drop(self.attrs.by_attr.remove(&k)),
            Some(_) => {}
            None => {
                for k in [(el.index(), false), (el.index(), true)] {
                    if self.attrs.maps.get(&k) == Some(&r) {
                        self.attrs.maps.remove(&k);
                    }
                }
            }
        }
        self.disown(el, r);
    }

    /// `n`'s wrapper is gone, and the members it held with it.
    pub(crate) fn drop_owned(&mut self, n: NodeId) {
        for r in self.attrs.owned.get(&n.index()).into_iter().flatten() {
            if let Some(mut m) = self.attrs.edges.remove(r) {
                m.forget();
            }
        }
    }

    pub(crate) fn trace_owned(&self, n: NodeId, t: &mut E::Tracer) {
        for r in self.attrs.owned.get(&n.index()).into_iter().flatten() {
            if let Some(m) = self.attrs.edges.get(r) {
                m.trace(t);
            }
        }
    }

    pub(crate) fn unlink_owned(&mut self, n: NodeId, u: &mut E::Unlinker) {
        for r in self.attrs.owned.get(&n.index()).into_iter().flatten() {
            if let Some(mut m) = self.attrs.edges.remove(r) {
                m.unlink(u);
            }
        }
    }

    pub(crate) fn close_owned(&mut self, e: &mut E) {
        for (_, mut m) in std::mem::take(&mut self.attrs.edges) {
            m.close(e);
        }
    }
}
