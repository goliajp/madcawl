//! What an event holds: its target, the target being called and the
//! path of a dispatch, each by a count of its own; and, on an engine
//! that places wrappers, its target's wrapper as a member.

use madcawl_js::Engine;

use crate::convert::IntoJs;
use crate::event::{EventState, Held, Target};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

impl<E: Engine> DomHost<E> {
    pub(crate) fn hold_target(&mut self, t: Target) -> Held {
        match t {
            Target::Node(n) => Held::Node(self.dom.hold(n)),
            Target::Side(r) => {
                self.sides.inc(r);
                Held::Side(r)
            }
            Target::Window => Held::Window,
        }
    }

    /// Give back what `held` holds.
    pub(crate) fn let_go_held(&mut self, held: Held) {
        match held {
            Held::Node(n) => self.dom.let_go(n),
            Held::Side(r) => self.sides.dec(r),
            Held::Window => {}
        }
    }

    pub(crate) fn set_held(&mut self, r: SideRef, which: fn(&mut EventState) -> &mut Option<Held>, t: Option<Target>) {
        let new = t.map(|t| self.hold_target(t));
        if let Some(old) = std::mem::replace(which(self.event_mut(r)), new) {
            self.let_go_held(old);
        }
    }

    /// Set or clear the target of event `r`; the wrapper of the target
    /// it had is no longer kept by the event.
    pub(crate) fn set_target(&mut self, e: &mut E, r: SideRef, t: Option<Target>) {
        if let Some(mut m) = self.target_edges.remove(&r) {
            // the event's wrapper is the caller's, alive
            m.clear(e);
        }
        self.set_held(r, |ev| &mut ev.target, t);
    }

    /// On an engine that places wrappers, keep the wrapper of the target
    /// of event `r` as a member of the event's wrapper `event` (of a
    /// live collection: its root's wrapper; of a fixed one: an array of
    /// the wrappers of its nodes outside the document; of a controller:
    /// its signal's wrapper, which is then the same object while the
    /// controller's lives; of an `Attr` or `attributes` map: its
    /// element's wrapper): script
    /// that reaches the event reaches the target's tree, with what it
    /// put there, and a cycle back through script (a listener keeping
    /// the event, the target keeping it in a property) is one the
    /// engine's collector sees and frees. The target's wrapper moves
    /// with its node from tree to tree, so whatever tree the target is
    /// in is the one kept. A fixed collection's node that was in the
    /// document when the collection was made gets no wrapper for it:
    /// the document's wrappers live with the realm, and making one for
    /// each node found would keep them all; it gets one when it leaves
    /// the document (`fixed.rs`).
    pub(crate) fn keep_target(&mut self, e: &mut E, r: SideRef, event: E::Value) {
        if !E::PLACES_WRAPPERS {
            return;
        }
        if let Some(mut m) = self.target_edges.remove(&r) {
            m.clear(e);
        }
        let w = match &self.sides.get(r).data {
            SideData::Controller(s) => s.target().into_js(e, self),
            SideData::Attr(_) | SideData::AttrMap(_) => match self.owning_element(r) {
                Some(el) => self.wrap(e, el),
                None => return,
            },
            SideData::Event(ev) => match ev.target.as_ref().map(Held::target) {
                Some(t @ (Target::Node(_) | Target::Side(_))) => t.into_js(e, self),
                _ => return,
            },
            SideData::List(l) => match l.root.as_ref().map(Held::target) {
                Some(t) => t.into_js(e, self),
                None => {
                    let (inside, nodes): (Vec<_>, Vec<_>) = self.list_nodes(r).into_iter().partition(|&n| self.dom.in_main_tree(n));
                    self.wait_for_removal(r, &inside);
                    if nodes.is_empty() {
                        return;
                    }
                    let items = nodes.into_iter().map(|n| self.wrap(e, n)).collect();
                    crate::convert::array_of(e, self, items)
                }
            },
            _ => return,
        };
        let m = madcawl_js::Member::of(e, event, w.get());
        e.release(w);
        self.target_edges.insert(r, m);
    }
}
