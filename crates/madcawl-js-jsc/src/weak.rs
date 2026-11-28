//! Weak handles and WindowProxy objects, both made of the language's
//! own objects: a weak handle to an object is a `WeakRef`, and a
//! WindowProxy is a `Proxy` that forwards to the `g` of a record the
//! adapter keeps.

use madcawl_js::Owned;

use crate::ffi;
use crate::isolate::{Jsc, Realm, WeakSlot};
use crate::protos::Helper;
use crate::value::{THROWN, UNDEFINED, Value};

impl Jsc {
    pub(crate) fn weak_slot(&mut self, v: Value) -> u32 {
        // SAFETY: a type check on a live value
        let object = v.is_cell() && unsafe { ffi::JSValueGetType(self.base, v.raw()) } == ffi::kJSTypeObject;
        // with no live realm no `WeakRef` can be made, and no script
        // runs that could drop the object: the handle holds it
        let made = match self.live_realm() {
            Some(r) if object => self.call_helper(r, Helper::WeakRef, &[v]),
            _ => Ok(UNDEFINED),
        };
        let slot = match made {
            Ok(w) if w != UNDEFINED => {
                self.protect(w);
                WeakSlot::Ref(w)
            }
            // a primitive; or script was being stopped as the `WeakRef`
            // was made, and the handle holds its object until dropped
            r => {
                if r.is_err() {
                    self.after_stop();
                }
                self.protect(v);
                WeakSlot::Strong(v)
            }
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
        let r = match *self.weak[w as usize].as_ref().expect("a live weak handle") {
            WeakSlot::Strong(v) => return Some(self.take(v)),
            WeakSlot::Ref(r) => r,
        };
        let read = match self.live_realm() {
            Some(realm) => self.call_helper(realm, Helper::Deref, &[r]),
            None => self.deref_without_realm(r),
        };
        match read {
            Ok(v) => (v != UNDEFINED).then(|| self.take(v)),
            // script was being stopped: read as gone
            Err(_) => {
                self.after_stop();
                None
            }
        }
    }

    /// `r.deref()` once every realm is detached, through the first
    /// realm's helper, which took the original `deref` before any page
    /// script ran and which the isolate keeps.
    fn deref_without_realm(&mut self, r: Value) -> Result<Value, Value> {
        let mut exc = std::ptr::null();
        // SAFETY: a live function of this group and a live `WeakRef`
        let read = unsafe { ffi::JSObjectCallAsFunction(self.base, self.deref.0 as _, std::ptr::null_mut(), 1, &r.raw(), &mut exc) };
        if exc.is_null() { Ok(Value::from_ref(read)) } else { Err(Value::from_ref(exc)) }
    }

    pub(crate) fn weak_slot_drop(&mut self, w: u32) {
        let slot = self.weak[w as usize].take().expect("a live weak handle");
        let (WeakSlot::Ref(v) | WeakSlot::Strong(v)) = slot;
        self.unprotect(v);
        self.weak_free.push(w);
    }

    /// Thrown when script is halted: making the proxy runs script.
    pub(crate) fn window_proxy(&mut self, realm: Realm) -> Owned<Value> {
        let ctx = self.realm_ctx(realm);
        if self.halted() {
            return Jsc::owned(THROWN);
        }
        // SAFETY: a live realm
        let g = Value(unsafe { ffi::JSContextGetGlobalObject(ctx) } as u64);
        let pair = match self.call_helper(realm.0, Helper::Proxy, &[g]) {
            Ok(pair) => pair,
            Err(_) => {
                self.after_stop();
                return Jsc::owned(THROWN);
            }
        };
        // SAFETY: the two-element array the helper returned
        let (p, state) = unsafe {
            (
                Value::from_ref(ffi::JSObjectGetPropertyAtIndex(ctx, pair.0 as _, 0, std::ptr::null_mut())),
                Value::from_ref(ffi::JSObjectGetPropertyAtIndex(ctx, pair.0 as _, 1, std::ptr::null_mut())),
            )
        };
        self.protect(p);
        self.protect(state);
        self.proxies.insert(p.0, (state, realm));
        self.take(p)
    }

    /// A window of another origin as `realm` sees it (`Helper::CrossOrigin`).
    pub(crate) fn cross_origin(&mut self, realm: Realm, members: Value, deny: Value) -> Owned<Value> {
        self.realm_ctx(realm);
        if self.halted() {
            return Jsc::owned(THROWN);
        }
        match self.call_helper(realm.0, Helper::CrossOrigin, &[members, deny]) {
            Ok(o) => self.take(o),
            Err(_) => {
                self.after_stop();
                Jsc::owned(THROWN)
            }
        }
    }

    /// Point `proxy` at the global of `realm`: a plain property set on a
    /// record with no prototype, so nothing can stop or intercept it.
    pub(crate) fn retarget(&mut self, proxy: Value, realm: Realm) {
        let (state, _) = *self.proxies.get(&proxy.0).expect("a WindowProxy");
        let ctx = self.realm_ctx(realm);
        // SAFETY: a live realm
        let g = unsafe { ffi::JSContextGetGlobalObject(ctx) };
        let name = self.js_str_utf8("g");
        let mut exc = std::ptr::null();
        // SAFETY: the proxy's record, an ordinary object of this group
        unsafe { ffi::JSObjectSetProperty(self.base, state.0 as _, name.s, g as _, 0, &mut exc) };
        debug_assert!(exc.is_null(), "a data property on an object with no prototype");
        self.proxies.insert(proxy.0, (state, realm));
    }
}
