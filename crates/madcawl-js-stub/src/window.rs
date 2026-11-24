//! WindowProxy: one object per browsing context that forwards to the
//! global object of the realm its context shows now.

use madcawl_js::Owned;

use crate::cell::{Body, Cell};
use crate::isolate::{Realm, Stub};
use crate::value::Value;

impl Stub {
    pub(crate) fn window_proxy(&mut self, realm: Realm) -> Owned<Value> {
        let global = self.live_global(self.index(realm));
        self.retain_value(global);
        Stub::owned(self.alloc(self.index(realm), Body::Proxy { global, realm: self.index(realm) }))
    }

    pub(crate) fn retarget(&mut self, proxy: Value, realm: Realm) {
        assert!(self.proxy_realm(proxy).is_some(), "a WindowProxy");
        let global = self.live_global(self.index(realm));
        self.retain_value(global);
        // SAFETY: a live cell
        let Body::Proxy { global: g, realm: r } = &mut unsafe { Cell::at(proxy) }.body else {
            unreachable!()
        };
        let old = std::mem::replace(g, global);
        *r = self.index(realm);
        self.release_value(old);
    }

    pub(crate) fn proxy_realm(&self, v: Value) -> Option<Realm> {
        // SAFETY: a live cell, read only
        match v.is_cell().then(|| &unsafe { Cell::at(v) }.body) {
            Some(Body::Proxy { realm, .. }) => Some(Realm(*realm, self.id)),
            _ => None,
        }
    }

    /// The object a property access on `v` goes to: a WindowProxy's
    /// current global, otherwise `v`.
    pub(crate) fn target(&self, v: Value) -> Value {
        // SAFETY: a live cell, read only
        match v.is_cell().then(|| &unsafe { Cell::at(v) }.body) {
            Some(Body::Proxy { global, .. }) => *global,
            _ => v,
        }
    }
}
