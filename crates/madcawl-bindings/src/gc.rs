//! A node as a host object: the collector's callbacks and the wrapper
//! protocol.
//!
//! The counted edges out of a node are the DOM's (parent, children,
//! node document, template contents, inert document), its listeners
//! and the wrappers of its `childNodes` and `children` (member handles),
//! and its wrapper when preserved. Each is reported
//! once. A node in a document is never entered; leaving a document
//! makes it a candidate.

use std::marker::PhantomData;

use madcawl_dom::Edge;
use madcawl_js::{Engine, HostOps, HostRef, Owned, Tracer, Unlinker};

use crate::host::DomHost;

pub struct NodeOps<E>(PhantomData<E>);

impl<E: Engine> NodeOps<E> {
    pub const OPS: HostOps<E, DomHost<E>> = HostOps {
        traverse: traverse::<E>,
        unlink: unlink::<E>,
        can_skip: can_skip::<E>,
        wrapper_needs_preserve: preserve::<E>,
        wrapper_died: died::<E>,
    };
}

fn traverse<E: Engine>(h: &DomHost<E>, obj: HostRef, t: &mut E::Tracer) {
    let n = h.node(obj);
    h.dom.host_edges(n, |edge| {
        let (Edge::Parent(m) | Edge::Child(m) | Edge::Document(m) | Edge::Contents(m) | Edge::Inert(m)) =
            edge;
        t.host(h.host_ref(m));
    });
    for l in h.listeners_of(n) {
        l.callback.trace(t);
    }
    for elements in [false, true] {
        if let Some(m) = h.list_edges.get(&(n.index(), elements)) {
            m.trace(t);
        }
    }
    let w = h.dom.wrapper(n);
    if w & 1 != 0 {
        t.value(E::wrapper_value(w & !1));
    }
}

fn unlink<E: Engine>(h: &mut DomHost<E>, obj: HostRef, u: &mut E::Unlinker) {
    let n = h.node(obj);
    for mut l in h.take_listeners(n) {
        l.callback.unlink(u);
    }
    for elements in [false, true] {
        if let Some(mut m) = h.list_edges.remove(&(n.index(), elements)) {
            m.unlink(u);
        }
    }
    let w = h.dom.wrapper(n);
    if w & 1 != 0 {
        h.dom.set_wrapper_preserved(n, false);
        // SAFETY: the count the preserved edge held on the wrapper
        u.release(unsafe { Owned::from_raw(E::wrapper_value(w & !1)) });
    }
    h.dom.unlink_node(n);
}

fn can_skip<E: Engine>(h: &DomHost<E>, obj: HostRef) -> bool {
    h.dom.is_connected(h.node(obj))
}

fn preserve<E: Engine>(h: &mut DomHost<E>, obj: HostRef) {
    let n = h.node(obj);
    h.dom.set_wrapper_preserved(n, true);
}

fn died<E: Engine>(h: &mut DomHost<E>, obj: HostRef) {
    let n = h.node(obj);
    h.dom.clear_wrapper(n);
    h.drop_members(n);
}
