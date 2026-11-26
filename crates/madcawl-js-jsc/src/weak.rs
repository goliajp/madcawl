//! Weak handles and WindowProxy objects, both made of the language's
//! own objects: a weak handle to an object is a `WeakRef`, and a
//! WindowProxy is a `Proxy` whose target a function of the realm's
//! helpers changes.

use madcawl_js::Owned;

use crate::ffi;
use crate::isolate::{Jsc, Realm, WeakSlot};
use crate::protos::Helper;
use crate::value::{UNDEFINED, Value};

impl Jsc {
    pub(crate) fn weak_slot(&mut self, v: Value) -> u32 {
        // SAFETY: a type check on a live value
        let object = v.is_cell() && unsafe { ffi::JSValueGetType(self.base, v.raw()) } == ffi::kJSTypeObject;
        let slot = if object {
            let w = self.call_helper(self.current, Helper::WeakRef, &[v]).expect("making a WeakRef");
            self.protect(w);
            WeakSlot::Ref(w)
        } else {
            self.protect(v);
            WeakSlot::Strong(v)
        };
        match self.weak_free.pop() {
            Some(i) => {
                self.weak[i as usize] = Some(slot);
                i
            }
            None => {
                self.weak.push(Some(slot));
                (self.weak.len() - 1) as u32
            }
        }
    }

    pub(crate) fn weak_read(&mut self, w: u32) -> Option<Owned<Value>> {
        match *self.weak[w as usize].as_ref().expect("a live weak handle") {
            WeakSlot::Strong(v) => Some(self.take(v)),
            WeakSlot::Ref(r) => {
                let v = self.call_helper(self.current, Helper::Deref, &[r]).expect("reading a WeakRef");
                (v != UNDEFINED).then(|| self.take(v))
            }
        }
    }

    pub(crate) fn weak_slot_drop(&mut self, w: u32) {
        let slot = self.weak[w as usize].take().expect("a live weak handle");
        let (WeakSlot::Ref(v) | WeakSlot::Strong(v)) = slot;
        self.unprotect(v);
        self.weak_free.push(w);
    }

    pub(crate) fn window_proxy(&mut self, realm: Realm) -> Owned<Value> {
        let ctx = self.realm_ctx(realm);
        // SAFETY: a live realm
        let g = Value(unsafe { ffi::JSContextGetGlobalObject(ctx) } as u64);
        let pair = self.call_helper(realm.0, Helper::Proxy, &[g]).expect("making a WindowProxy");
        // SAFETY: the two-element array the helper returned
        let (p, f) = unsafe {
            (
                Value::from_ref(ffi::JSObjectGetPropertyAtIndex(ctx, pair.0 as _, 0, std::ptr::null_mut())),
                Value::from_ref(ffi::JSObjectGetPropertyAtIndex(ctx, pair.0 as _, 1, std::ptr::null_mut())),
            )
        };
        self.protect(p);
        self.protect(f);
        self.proxies.insert(p.0, (f, realm));
        self.take(p)
    }

    pub(crate) fn retarget(&mut self, proxy: Value, realm: Realm) {
        let (f, _) = *self.proxies.get(&proxy.0).expect("a WindowProxy");
        let ctx = self.realm_ctx(realm);
        // SAFETY: a live realm
        let g = Value(unsafe { ffi::JSContextGetGlobalObject(ctx) } as u64);
        let mut exc = std::ptr::null();
        // SAFETY: the retargeting function of a live proxy
        unsafe { ffi::JSObjectCallAsFunction(ctx, f.0 as _, std::ptr::null_mut(), 1, &g.raw(), &mut exc) };
        assert!(exc.is_null(), "retargeting a WindowProxy");
        self.proxies.insert(proxy.0, (f, realm));
    }
}
