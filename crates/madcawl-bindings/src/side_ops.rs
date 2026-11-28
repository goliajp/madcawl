//! A side object as a host object: the collector's callbacks and the
//! wrapper protocol, as a node's (`gc.rs`).

use std::marker::PhantomData;

use madcawl_js::{Engine, HostOps, HostRef, Owned, Tracer, Unlinker};

use crate::event::Held;
use crate::host::DomHost;
use crate::side::SideRef;

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
    let r = side_of(h, obj);
    let s = h.sides.get(r);
    s.data.held(&mut |x| match x {
        Held::Node(n) => t.host(h.host_ref(n.id())),
        Held::Side(r) => t.host(r.host_ref()),
        Held::Window(_) => {}
    });
    for m in h.details.get(&r).into_iter().chain(h.target_edges.get(&r)).chain(h.fixed_edges(r)) {
        m.trace(t);
    }
    for l in h.side_listeners.get(&r).into_iter().flatten() {
        l.callback.trace(t);
    }
    if s.wrapper & 1 != 0 {
        t.value(E::wrapper_value(s.wrapper & !1));
    }
}

fn unlink<E: Engine>(h: &mut DomHost<E>, obj: HostRef, u: &mut E::Unlinker) {
    let r = side_of(h, obj);
    for mut m in h.details.remove(&r).into_iter().chain(h.target_edges.remove(&r)) {
        m.unlink(u);
    }
    h.forget_fixed(r);
    h.forget_owned(r, true);
    for mut l in h.side_listeners.remove(&r).unwrap_or_default() {
        l.callback.unlink(u);
    }
    let mut held = Vec::new();
    let s = h.sides.get_mut(r);
    s.data.release(&mut held);
    if s.wrapper & 1 != 0 {
        s.wrapper &= !1;
        // SAFETY: the count the preserved edge held on the wrapper
        u.release(unsafe { Owned::from_raw(E::wrapper_value(s.wrapper)) });
    }
    for x in held {
        h.let_go_held(x);
    }
}

fn preserve<E: Engine>(h: &mut DomHost<E>, obj: HostRef) {
    let r = side_of(h, obj);
    h.sides.get_mut(r).wrapper |= 1;
}

fn died<E: Engine>(h: &mut DomHost<E>, obj: HostRef) {
    let r = side_of(h, obj);
    h.drop_side_members(r);
    let s = h.sides.get_mut(r);
    assert_eq!(s.wrapper & 1, 0, "a preserved wrapper does not die");
    s.wrapper = 0;
    h.sides.dec(r);
}

impl<E: Engine> DomHost<E> {
    /// How many host objects that are not nodes are alive.
    pub fn side_objects(&self) -> usize {
        self.sides.len()
    }
}
