//! Wrappers: made with a record the host's wrapper word points at,
//! protected until the host places them in a detached tree
//! (`groups.rs`), and never announced as preserved: the protection, or
//! the tree's group, is what keeps a wrapper, not a count the host holds.

use std::any::TypeId;

use madcawl_js::{ClassDef, Host, HostRef, Owned};

use crate::ffi;
use crate::isolate::{Jsc, Realm};
use crate::objects::WrapRec;
use crate::protos::Helper;
use crate::value::{UNDEFINED, Value};

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
            gone: false,
            dead: false,
            orphan: false,
            strong: true,
            weak: Value(0),
            grouped: false,
            container: Value(0),
            members: Vec::new(),
            shared: self.shared,
        }));
        let list = class.chain().find_map(|c| c.interceptors).and_then(|x| {
            Some(crate::lists::ListCalls::of::<H>(x.index?.get(), x.length?.get(), x.named.map(|f| f.get())))
        });
        let jsclass = self.classes.wrapper_of(class.id.0, class.name, list);
        // SAFETY: the record lives until the host is told the wrapper died
        let w = Value(unsafe { ffi::JSObjectMake(ctx, jsclass, rec.cast()) } as u64);
        // one count for the caller, one that keeps it while strong
        self.protect(w);
        self.protect(w);
        // SAFETY: just made; a live prototype of the same realm
        unsafe {
            (*rec).js = w;
            ffi::JSObjectSetPrototype(ctx, w.0 as _, proto.raw());
        }
        self.realms[realm.0 as usize].wrappers.insert(rec as usize);
        Jsc::owned(w)
    }

    /// The wrapper of `word` belongs to `realm` from now on. A group it
    /// is in stays in the old realm's map until the host places it again,
    /// which it does for a node that moved.
    pub(crate) fn rehome(&mut self, word: u64, realm: Realm) {
        let to = self.index(realm);
        // SAFETY: a live record the host holds the word of
        let rec = unsafe { &mut *(word as *mut WrapRec) };
        assert!(!rec.gone && !self.realms[to as usize].detached, "a wrapper moves between live realms");
        if rec.realm == to {
            return;
        }
        self.realms[rec.realm as usize].wrappers.remove(&(word as usize));
        self.realms[to as usize].wrappers.insert(word as usize);
        rec.realm = to;
    }

    pub(crate) fn word_of(wrapper: Value) -> u64 {
        // SAFETY: a live wrapper, whose private data is its record
        (unsafe { ffi::JSObjectGetPrivate(wrapper.0 as _) }) as u64
    }

    /// A strong wrapper is alive. Any other may have been found dead by
    /// a collection whose sweep has not reached it yet, which only its
    /// `WeakRef` tells: protecting such an object would keep a pointer
    /// to memory the sweep frees. One whose realm was detached is
    /// refused: it has neither, and its slot may hold another realm.
    pub(crate) fn upgrade(&mut self, word: u64) -> Option<Owned<Value>> {
        // SAFETY: a record is freed only once the host was told, or after
        // it was refused here
        let rec = unsafe { &mut *(word as *mut WrapRec) };
        assert!(!rec.orphan, "a refused wrapper's word was cleared");
        if rec.dead || rec.gone {
            rec.orphan = true;
            return None;
        }
        if rec.strong {
            return Some(self.take(rec.js));
        }
        match self.call_helper(rec.realm, Helper::Deref, &[rec.weak]) {
            Ok(v) if v != UNDEFINED => Some(self.take(v)),
            r => {
                // dead; or script was being stopped and whether it lives
                // is not known: refused either way, so the host never
                // protects it and script holding it reaches nothing
                if r.is_err() {
                    self.after_stop();
                }
                rec.orphan = true;
                None
            }
        }
    }

    pub(crate) fn host_object<H: Host<Jsc>>(&self, v: Value, class: &'static ClassDef<Jsc, H>) -> Option<HostRef> {
        // SAFETY: a type check on a live value
        if !v.is_cell() || !unsafe { ffi::JSValueIsObjectOfClass(self.base, v.raw(), self.classes.wrapper) } {
            return None;
        }
        // SAFETY: a wrapper's private data is its record
        let rec = unsafe { &*ffi::JSObjectGetPrivate(v.0 as _).cast::<WrapRec>() };
        let live = !rec.gone && !rec.orphan;
        (live && rec.host_type == TypeId::of::<H>() && class.contains(rec.id)).then_some(rec.obj)
    }

    /// Tell the host which wrappers died; free their records. The
    /// wrappers of a detached realm belonged to a host that is gone,
    /// which is not this one: those are freed untold.
    pub(crate) fn drain<H: Host<Jsc>>(&mut self, host: &mut H) -> usize {
        let dead = self.shared().notices.take();
        let mut n = 0;
        for rec in dead {
            // SAFETY: a finalized wrapper's record, freed only here
            let r = unsafe { Box::from_raw(rec) };
            self.unprotect(r.weak);
            // the slot may hold another realm, whose set never had it
            self.realms[r.realm as usize].wrappers.remove(&(rec as usize));
            if !r.orphan && !r.gone {
                (class_of::<H>(&r).host_ops().wrapper_died)(host, r.obj);
                n += 1;
            }
        }
        n
    }
}
