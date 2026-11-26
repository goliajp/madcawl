//! Host objects that are not nodes: events, collections and the like. Each is a box that
//! starts with the engine's 8-byte object header and a wrapper word, as
//! a node slot does, so the engine treats it as it treats a node: the
//! wrapper holds it by a count, and the collector asks for its edges.
//! The nodes it points at it holds through counted handles, which it
//! reports.

use std::collections::HashMap;

use madcawl_js::{Engine, HostRef, Owned, Tree};
use madcawl_str::abi;

use crate::event::{EventState, Held};
use crate::host::DomHost;

/// A side object, by its address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SideRef(usize);

#[repr(C)]
pub(crate) struct Side {
    header: u64,
    /// As a node's: the wrapper's pointer, with the preserved bit.
    pub wrapper: u64,
    pub data: SideData,
}

pub(crate) enum SideData {
    Event(EventState),
    /// An `EventTarget` script made: its listeners are in the host's
    /// `side_listeners`.
    Target,
    DomException(crate::impls::dom_exception::DomException),
    /// A `NodeList` or `HTMLCollection` (`lists.rs`).
    List(crate::lists::ListState),
}

impl SideData {
    /// The targets it holds by count, for `traverse`.
    pub(crate) fn held(&self, f: &mut impl FnMut(&Held)) {
        match self {
            SideData::Event(ev) => ev.held(f),
            SideData::List(l) => l.held(f),
            _ => {}
        }
    }

    pub(crate) fn release(&mut self, out: &mut Vec<Held>) {
        match self {
            SideData::Event(ev) => ev.release(out),
            SideData::List(l) => l.release(out),
            _ => {}
        }
    }
}

#[derive(Default)]
pub(crate) struct Sides {
    map: HashMap<usize, Box<Side>>,
    /// Whose count reached 0, freed when the host settles.
    zero: Vec<usize>,
    /// Whose count dropped and stayed above 0: cycle candidates.
    suspects: Vec<usize>,
}

impl Sides {
    /// A new side object, with no count: freed at the next settle unless
    /// something holds it by then.
    pub fn add(&mut self, data: SideData) -> SideRef {
        let b = Box::new(Side { header: abi::with_refcount(abi::header(0, 0), 0), wrapper: 0, data });
        let addr = std::ptr::from_ref::<Side>(&b) as usize;
        self.map.insert(addr, b);
        self.zero.push(addr);
        SideRef(addr)
    }

    pub fn get(&self, r: SideRef) -> &Side {
        self.map.get(&r.0).expect("a live side object")
    }

    pub fn get_mut(&mut self, r: SideRef) -> &mut Side {
        self.map.get_mut(&r.0).expect("a live side object")
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn of(&self, obj: HostRef) -> Option<SideRef> {
        self.map.contains_key(&obj.addr()).then_some(SideRef(obj.addr()))
    }

    pub fn inc(&mut self, r: SideRef) {
        let s = self.get_mut(r);
        s.header = abi::with_refcount(s.header, abi::refcount(s.header) + 1);
    }

    pub fn dec(&mut self, r: SideRef) {
        let s = self.get_mut(r);
        let n = abi::refcount(s.header) - 1;
        s.header = abi::with_refcount(s.header, n);
        if n == 0 { self.zero.push(r.0) } else { self.suspects.push(r.0) }
    }
}

impl SideRef {
    pub(crate) fn host_ref(self) -> HostRef {
        HostRef::new(self.0).expect("a box is never at 0")
    }
}

impl<E: Engine> DomHost<E> {
    /// How many host objects that are not nodes are alive.
    pub fn side_objects(&self) -> usize {
        self.sides.len()
    }

    /// The wrapper of side object `r`, made if it has none or the one it
    /// has is dead, with a count for the caller.
    pub(crate) fn wrap_side(&mut self, e: &mut E, r: SideRef) -> Owned<E::Value> {
        let w = self.sides.get(r).wrapper;
        if w != 0 {
            if let Some(v) = e.wrapper_upgrade(w & !1) {
                return v;
            }
            self.side_wrapper_gone(e, r);
        }
        let obj = r.host_ref();
        let v = e.new_wrapper(self.realm, crate::host::side_class::<E>(&self.sides.get(r).data), obj);
        let word = E::wrapper_word(v.get());
        self.sides.get_mut(r).wrapper = word;
        self.sides.inc(r);
        if E::PLACES_WRAPPERS {
            if !self.place_side(e, r) {
                self.unplaced_sides.push(r);
            }
            self.keep_target(e, r, v.get());
            self.keep_child_list(e, r, v.get());
        }
        v
    }

    /// The wrapper of side object `r` with a count, when `r` is still a
    /// side object and its wrapper lives; one found dead is let go of.
    pub(crate) fn live_side_wrapper(&mut self, e: &mut E, r: SideRef) -> Option<Owned<E::Value>> {
        let w = self.sides.of(r.host_ref()).map_or(0, |r| self.sides.get(r).wrapper);
        if w == 0 {
            return None;
        }
        let v = e.wrapper_upgrade(w & !1);
        if v.is_none() {
            self.side_wrapper_gone(e, r);
        }
        v
    }

    /// Place the wrapper of `r` in no tree: it lives while script holds
    /// it. `false` when the engine could not place it now.
    fn place_side(&mut self, e: &mut E, r: SideRef) -> bool {
        let word = self.sides.get(r).wrapper & !1;
        let mut refused = Vec::new();
        if !e.place_wrappers(Tree::New, &[word], &mut refused) {
            return false;
        }
        if !refused.is_empty() {
            self.side_wrapper_gone(e, r);
        }
        true
    }

    /// Place the side wrappers the engine could not place before.
    pub(crate) fn place_sides(&mut self, e: &mut E) {
        for r in std::mem::take(&mut self.unplaced_sides) {
            let live = self.sides.map.get(&r.0).is_some_and(|s| s.wrapper != 0);
            if live && !self.place_side(e, r) {
                self.unplaced_sides.push(r);
            }
        }
    }

    /// The wrapper of `r` is gone or refused: clear its word and its count.
    fn side_wrapper_gone(&mut self, e: &mut E, r: SideRef) {
        self.drop_side_members(r);
        let w = std::mem::take(&mut self.sides.get_mut(r).wrapper);
        if w & 1 != 0 {
            // SAFETY: the count the preserved edge held
            e.release(unsafe { Owned::from_raw(E::wrapper_value(w & !1)) });
        }
        self.sides.dec(r);
    }

    /// Free the side objects no one counts, give back what they held,
    /// and pass on the candidates; whether anything changed.
    pub(crate) fn settle_sides(&mut self, e: &mut E) -> bool {
        let mut held = Vec::new();
        let zero = std::mem::take(&mut self.sides.zero);
        let changed = !zero.is_empty();
        for addr in zero {
            let live = self.sides.map.get(&addr).is_some_and(|s| abi::refcount(s.header) == 0);
            if live {
                self.forget_list(SideRef(addr));
            }
            if live && let Some(mut s) = self.sides.map.remove(&addr) {
                s.data.release(&mut held);
                if let Some(mut d) = self.details.remove(&SideRef(addr)) {
                    d.clear(e);
                }
                for mut l in self.side_listeners.remove(&SideRef(addr)).unwrap_or_default() {
                    l.callback.clear(e);
                }
                e.forget(HostRef::new(addr).expect("nonzero"));
            }
        }
        for x in held {
            self.let_go_held(x);
        }
        for addr in std::mem::take(&mut self.sides.suspects) {
            if self.sides.map.contains_key(&addr) {
                e.suspect(HostRef::new(addr).expect("nonzero"));
            }
        }
        changed
    }

    /// `r`'s wrapper is gone: on an engine that keeps members beside the
    /// wrapper, so are its members.
    pub(crate) fn drop_side_members(&mut self, r: SideRef) {
        if !E::PLACES_WRAPPERS {
            return;
        }
        for mut m in self.details.remove(&r).into_iter().chain(self.target_edges.remove(&r)) {
            m.forget();
        }
        self.forget_fixed(r);
        for mut l in self.side_listeners.remove(&r).unwrap_or_default() {
            l.callback.forget();
        }
    }

    /// Give back every side object's preserved wrapper edge, its members
    /// and the nodes it holds: the host is closing.
    pub(crate) fn close_sides(&mut self, e: &mut E) {
        self.close_fixed(e);
        let edges = std::mem::take(&mut self.target_edges).into_values().chain(std::mem::take(&mut self.list_edges).into_values());
        for mut m in std::mem::take(&mut self.details).into_values().chain(edges) {
            m.close(e);
        }
        for mut l in std::mem::take(&mut self.side_listeners).into_values().flatten() {
            l.callback.close(e);
        }
        let mut held = Vec::new();
        for s in self.sides.map.values_mut() {
            s.data.release(&mut held);
            if s.wrapper & 1 != 0 {
                s.wrapper &= !1;
                // SAFETY: the count the preserved edge held
                e.release(unsafe { Owned::from_raw(E::wrapper_value(s.wrapper)) });
            }
        }
        for x in held {
            self.let_go_held(x);
        }
    }
}
