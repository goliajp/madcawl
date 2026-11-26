//! Host objects that are not nodes: events so far. Each is a box that
//! starts with the engine's 8-byte object header and a wrapper word, as
//! a node slot does, so the engine treats it as it treats a node: the
//! wrapper holds it by a count, and the collector asks for its edges.
//! The nodes it points at it holds through counted handles, which it
//! reports.

use std::collections::HashMap;
use std::marker::PhantomData;

use madcawl_dom::{HeldNode, NodeId};
use madcawl_js::{Engine, HostOps, HostRef, Owned, Tracer, Tree, Unlinker};
use madcawl_str::abi;

use crate::event::EventState;
use crate::host::DomHost;

/// A side object, by its address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SideRef(usize);

#[repr(C)]
pub(crate) struct Side {
    header: u64,
    /// As a node's: the wrapper's pointer, with the preserved bit.
    wrapper: u64,
    pub data: SideData,
}

pub(crate) enum SideData {
    Event(EventState),
}

impl SideData {
    /// The nodes it holds, for `traverse`.
    fn nodes(&self, f: &mut impl FnMut(NodeId)) {
        match self {
            SideData::Event(ev) => ev.nodes(f),
        }
    }

    fn release(&mut self, out: &mut Vec<HeldNode>) {
        match self {
            SideData::Event(ev) => ev.release(out),
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
            // in no tree: it lives while script holds it
            let mut refused = Vec::new();
            e.place_wrappers(Tree::New, &[word], &mut refused);
            if !refused.is_empty() {
                self.side_wrapper_gone(e, r);
            }
        }
        v
    }

    /// The wrapper of `r` is gone or refused: clear its word and its count.
    fn side_wrapper_gone(&mut self, e: &mut E, r: SideRef) {
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
            if live && let Some(mut s) = self.sides.map.remove(&addr) {
                s.data.release(&mut held);
                e.forget(HostRef::new(addr).expect("nonzero"));
            }
        }
        for n in held {
            self.dom.let_go(n);
        }
        for addr in std::mem::take(&mut self.sides.suspects) {
            if self.sides.map.contains_key(&addr) {
                e.suspect(HostRef::new(addr).expect("nonzero"));
            }
        }
        changed
    }

    /// Give back every side object's preserved wrapper edge and the
    /// nodes it holds: the host is closing.
    pub(crate) fn close_sides(&mut self, e: &mut E) {
        let mut held = Vec::new();
        for s in self.sides.map.values_mut() {
            s.data.release(&mut held);
            if s.wrapper & 1 != 0 {
                s.wrapper &= !1;
                // SAFETY: the count the preserved edge held
                e.release(unsafe { Owned::from_raw(E::wrapper_value(s.wrapper)) });
            }
        }
        for n in held {
            self.dom.let_go(n);
        }
    }
}

pub struct SideOps<E>(PhantomData<E>);

impl<E: Engine> SideOps<E> {
    pub const OPS: HostOps<E, DomHost<E>> = HostOps {
        traverse: traverse::<E>,
        unlink: unlink::<E>,
        can_skip: |_, _| false,
        wrapper_needs_preserve: preserve::<E>,
        wrapper_died: died::<E>,
    };
}

fn side_of<E: Engine>(h: &DomHost<E>, obj: HostRef) -> SideRef {
    h.sides.of(obj).expect("a live side object")
}

fn traverse<E: Engine>(h: &DomHost<E>, obj: HostRef, t: &mut E::Tracer) {
    let s = h.sides.get(side_of(h, obj));
    s.data.nodes(&mut |n| t.host(h.host_ref(n)));
    if s.wrapper & 1 != 0 {
        t.value(E::wrapper_value(s.wrapper & !1));
    }
}

fn unlink<E: Engine>(h: &mut DomHost<E>, obj: HostRef, u: &mut E::Unlinker) {
    let r = side_of(h, obj);
    let mut held = Vec::new();
    let s = h.sides.get_mut(r);
    s.data.release(&mut held);
    if s.wrapper & 1 != 0 {
        s.wrapper &= !1;
        // SAFETY: the count the preserved edge held on the wrapper
        u.release(unsafe { Owned::from_raw(E::wrapper_value(s.wrapper)) });
    }
    for n in held {
        h.dom.let_go(n);
    }
}

fn preserve<E: Engine>(h: &mut DomHost<E>, obj: HostRef) {
    let r = side_of(h, obj);
    h.sides.get_mut(r).wrapper |= 1;
}

fn died<E: Engine>(h: &mut DomHost<E>, obj: HostRef) {
    let r = side_of(h, obj);
    let s = h.sides.get_mut(r);
    assert_eq!(s.wrapper & 1, 0, "a preserved wrapper does not die");
    s.wrapper = 0;
    h.sides.dec(r);
}
