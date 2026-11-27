//! A side object's members, and giving back what every side object
//! holds when the host closes.

use madcawl_js::{Engine, Owned};

use crate::host::DomHost;
use crate::side::SideRef;

impl<E: Engine> DomHost<E> {
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
        self.forget_owned(r, false);
        for mut l in self.side_listeners.remove(&r).unwrap_or_default() {
            l.callback.forget();
        }
    }

    /// Give back every side object's preserved wrapper edge, its members
    /// and the nodes it holds: the host is closing.
    pub(crate) fn close_sides(&mut self, e: &mut E) {
        self.close_fixed(e);
        self.close_owned(e);
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
