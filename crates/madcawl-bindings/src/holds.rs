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
    /// collection: its root's wrapper, or an array of a fixed one's
    /// nodes' wrappers, the same way): script
    /// that reaches the event reaches the target's tree, with what it
    /// put there, and a cycle back through script (a listener keeping
    /// the event, the target keeping it in a property) is one the
    /// engine's collector sees and frees. The target's wrapper moves
    /// with its node from tree to tree, so whatever tree the target is
    /// in is the one kept.
    pub(crate) fn keep_target(&mut self, e: &mut E, r: SideRef, event: E::Value) {
        if !E::PLACES_WRAPPERS {
            return;
        }
        if let Some(mut m) = self.target_edges.remove(&r) {
            m.clear(e);
        }
        let w = match &self.sides.get(r).data {
            SideData::Event(ev) => match ev.target.as_ref().map(Held::target) {
                Some(t @ (Target::Node(_) | Target::Side(_))) => t.into_js(e, self),
                _ => return,
            },
            SideData::List(l) => match l.root.as_ref().map(Held::target) {
                Some(t) => t.into_js(e, self),
                None => {
                    let nodes = self.list_nodes(r);
                    let items = nodes.into_iter().map(|n| self.wrap(e, n)).collect();
                    crate::convert::array_of(e, self, items)
                }
            },
            _ => return,
        };
        // a page that replaced `Array.of` gets a fixed collection whose
        // nodes' trees it does not keep
        if E::is_thrown(w.get()) {
            let _sentinel = w.into_raw();
            if let Some(x) = e.take_exception() {
                e.release(x);
            }
            return;
        }
        let m = madcawl_js::Member::of(e, event, w.get());
        e.release(w);
        self.target_edges.insert(r, m);
    }
}
