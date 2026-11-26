//! Wrappers: made with a record the host's wrapper word points at,
//! preserved from the start (see `collect.rs`), and announced to the
//! host as preserved when it next settles.

use std::any::TypeId;

use madcawl_js::{ClassDef, Host, HostRef, Owned};

use crate::ffi;
use crate::isolate::{Jsc, Realm};
use crate::objects::WrapRec;
use crate::value::Value;

/// The class table a record names, as host type `H`.
fn class_of<H: 'static>(rec: &WrapRec) -> &'static ClassDef<Jsc, H> {
    assert_eq!(rec.host_type, TypeId::of::<H>(), "one host type per isolate");
    // SAFETY: erased from this type, checked above; class tables are static
    unsafe { &*rec.def.cast::<ClassDef<Jsc, H>>() }
}

impl Jsc {
    pub(crate) fn make_wrapper<H: Host<Jsc>>(&mut self, realm: Realm, class: &'static ClassDef<Jsc, H>, obj: HostRef) -> Owned<Value> {
        let ctx = self.realm_ctx(realm);
        let (_, proto) = self.class_objects(realm.0, class);
        let rec = Box::into_raw(Box::new(WrapRec {
            obj,
            def: std::ptr::from_ref(class).cast(),
            id: class.id,
            host_type: TypeId::of::<H>(),
            js: Value(0),
            realm: realm.0,
            dead: false,
            orphan: false,
            shared: self.shared,
        }));
        // SAFETY: the record lives until the host is told the wrapper died
        let w = Value(unsafe { ffi::JSObjectMake(ctx, self.classes.wrapper, rec.cast()) } as u64);
        // one count for the caller, one for the preserved edge
        self.protect(w);
        self.protect(w);
        // SAFETY: just made; a live prototype of the same realm
        unsafe {
            (*rec).js = w;
            ffi::JSObjectSetPrototype(ctx, w.0 as _, proto.raw());
        }
        self.unannounced.push(rec);
        Jsc::owned(w)
    }

    pub(crate) fn word_of(wrapper: Value) -> u64 {
        // SAFETY: a live wrapper, whose private data is its record
        (unsafe { ffi::JSObjectGetPrivate(wrapper.0 as _) }) as u64
    }

    pub(crate) fn upgrade(&mut self, word: u64) -> Option<Owned<Value>> {
        // SAFETY: a record is freed only once the host was told, or after
        // it was refused here
        let rec = unsafe { &mut *(word as *mut WrapRec) };
        assert!(!rec.orphan, "a refused wrapper's word was cleared");
        if rec.dead {
            rec.orphan = true;
            return None;
        }
        Some(self.take(rec.js))
    }

    pub(crate) fn host_object<H: Host<Jsc>>(&self, v: Value, class: &'static ClassDef<Jsc, H>) -> Option<HostRef> {
        // SAFETY: a type check on a live value
        if !v.is_cell() || !unsafe { ffi::JSValueIsObjectOfClass(self.base, v.raw(), self.classes.wrapper) } {
            return None;
        }
        // SAFETY: a wrapper's private data is its record
        let rec = unsafe { &*ffi::JSObjectGetPrivate(v.0 as _).cast::<WrapRec>() };
        let live = !self.realms[rec.realm as usize].detached;
        (live && rec.host_type == TypeId::of::<H>() && class.contains(rec.id)).then_some(rec.obj)
    }

    /// Tell the host which new wrappers are preserved and which died;
    /// free the records of the dead. The wrappers of a detached realm
    /// belonged to a host that is gone, which is not this one: a new one
    /// loses its preserved count here, a dead one is freed untold.
    pub(crate) fn drain<H: Host<Jsc>>(&mut self, host: &mut H) -> usize {
        for rec in std::mem::take(&mut self.unannounced) {
            // SAFETY: a record whose wrapper is preserved, so alive
            let rec = unsafe { &*rec };
            if self.realms[rec.realm as usize].detached {
                self.unprotect(rec.js);
                continue;
            }
            (class_of::<H>(rec).host_ops().wrapper_needs_preserve)(host, rec.obj);
        }
        let dead = self.shared().notices.take();
        let mut n = 0;
        for rec in dead {
            // SAFETY: a finalized wrapper's record, freed only here
            let r = unsafe { Box::from_raw(rec) };
            if !r.orphan && !self.realms[r.realm as usize].detached {
                (class_of::<H>(&r).host_ops().wrapper_died)(host, r.obj);
                n += 1;
            }
        }
        n
    }
}
