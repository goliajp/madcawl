//! Interface objects made when script first reads them: the global gets
//! an accessor per interface, in its place among the global's own keys,
//! and the first read makes the interface object (`protos.rs`) and turns
//! the accessor into the data property it stands for. A realm makes
//! only the interfaces its script names or its wrappers need.

use madcawl_js::{Bound, ClassDef, Ctx, Host};
use madcawl_str::DomString;

use crate::isolate::{Jsc, Realm};
use crate::objects::{FnKind, FnRec, make_function, thunk};
use crate::protos::Helper;
use crate::value::Value;

impl Jsc {
    pub(crate) fn lazy_interfaces<H: Host<Jsc>>(&mut self, realm: Realm, global: Value, classes: &'static [&'static ClassDef<Jsc, H>]) -> bool {
        let ctx = self.realm_ctx(realm);
        let r = realm.0 as usize;
        assert!(self.realms[r].lazy.0.is_null(), "one set of lazy interfaces per realm");
        self.realms[r].lazy = (classes.as_ptr().cast(), classes.len());
        let names: Vec<Value> = classes.iter().map(|c| self.string_of(&DomString::from_wtf8(c.name.as_bytes()))).collect();
        let list = self.array(ctx, &names);
        for v in names {
            self.unprotect(v);
        }
        // SAFETY: `make` reads its context as Jsc and H
        let f = unsafe { Bound::<madcawl_js::HostFn, Jsc, H>::host_fn(make::<H>) };
        let data = u64::from(realm.0) | u64::from(realm.2) << 32;
        let kind = FnKind::Host { f: f.get(), data, thunk: thunk::<H>, realm: realm.0, generation: realm.2 };
        let fproto = self.helper(realm.0, Helper::FunctionProto);
        let make = make_function(self, ctx, FnRec { kind, host_type: std::any::TypeId::of::<H>() }, fproto);
        let done = self.call_helper(realm.0, Helper::Lazy, &[global, list, make]);
        self.unprotect(list);
        self.unprotect(make);
        if done.is_err() {
            self.after_stop();
        }
        done.is_ok()
    }
}

/// `make(i)`: the interface object of the realm's `i`th lazy class,
/// made now if no wrapper or earlier read made it; the realm's slot and
/// generation are the function's data word. Throws a TypeError once
/// the realm is detached.
///
/// # Safety
/// As for any [`madcawl_js::HostFn`] bound to Jsc and `H`, made by
/// `lazy_interfaces` for a realm whose classes are of host type `H`.
unsafe extern "C" fn make<H: Host<Jsc>>(ctx: *mut Ctx, _this: u64, argv: *const u64, argc: usize) -> u64 {
    // SAFETY: the caller's
    let (e, _) = unsafe { Ctx::parts::<Jsc, H>(ctx) };
    // SAFETY: the context of this call
    let data = unsafe { (*ctx).data };
    let (realm, generation) = (data as u32, (data >> 32) as u32);
    // an accessor kept after its realm was let go: what it would make
    // its interface from is gone with the realm, whose slot may hold
    // another by now
    if e.realms[realm as usize].detached || e.realms[realm as usize].generation != generation {
        return e.throw_new(madcawl_js::ErrorKind::Type, "the interface's window is gone").into_raw().0;
    }
    // SAFETY: the helper passes one number
    let i = unsafe { <Jsc as madcawl_js::Engine>::values(argv, argc) }[0].as_number().expect("an index") as usize;
    let (ptr, len) = e.realms[realm as usize].lazy;
    assert!(i < len, "a lazy interface index in range");
    // SAFETY: erased in lazy_interfaces from a static slice of this host type
    let class = unsafe { std::slice::from_raw_parts(ptr.cast::<&'static ClassDef<Jsc, H>>(), len) }[i];
    let (iface, _) = e.class_objects(realm, class);
    e.protect(iface);
    iface.0
}
