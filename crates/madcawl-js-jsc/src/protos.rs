//! Realms: each a global context of the isolate's group, with a few
//! script helpers made once (defining accessors, making errors, queueing
//! microtasks, weak references, the WindowProxy, handler compilation)
//! and the interface and prototype objects of the host classes.

use std::collections::{HashMap, HashSet};

use madcawl_js::{ClassDef, Host};

use crate::ffi::{self, JSContextRef, JSGlobalContextRef, JSValueRef};
use crate::isolate::{Jsc, Realm};
use crate::objects::{FnKind, FnRec, make_function, thunk};
use crate::value::{UNDEFINED, Value};

pub(crate) use crate::helpers::{HELPERS, Helper};

pub(crate) struct RealmData {
    pub ctx: JSGlobalContextRef,
    pub detached: bool,
    /// Each host class's interface object and prototype object, by
    /// class id, made when first asked for; counted.
    pub protos: HashMap<u16, (Value, Value)>,
    pub helpers: Vec<Value>,
    /// The records of the realm's wrappers not yet freed.
    pub wrappers: HashSet<usize>,
}

impl Jsc {
    pub(crate) fn make_realm(&mut self) -> Realm {
        // SAFETY: a new context of this isolate's group
        let ctx = unsafe { ffi::JSGlobalContextCreateInGroup(self.group, std::ptr::null_mut()) };
        let src = self.js_str_utf8(HELPERS);
        let mut exc: JSValueRef = std::ptr::null();
        // SAFETY: the helper script on a live context
        let arr = unsafe { ffi::JSEvaluateScript(ctx, src.s, std::ptr::null_mut(), std::ptr::null_mut(), 1, &mut exc) };
        assert!(exc.is_null(), "the realm helpers compile");
        let helpers: Vec<Value> = (0..=Helper::LAST as u32)
            .map(|i| {
                // SAFETY: the array the script returned
                let v = Value::from_ref(unsafe {
                    ffi::JSObjectGetPropertyAtIndex(ctx, arr as _, i, std::ptr::null_mut())
                });
                self.protect(v);
                v
            })
            .collect();
        let r = self.realms.len() as u32;
        if r == 0 {
            self.deref = helpers[Helper::Deref as usize];
            self.protect(self.deref);
        }
        self.realms.push(RealmData { ctx, detached: false, protos: HashMap::new(), helpers, wrappers: HashSet::new() });
        Realm(r, self.id)
    }

    /// A function object calling host function `f` with `data`.
    pub(crate) fn host_function<H: Host<Jsc>>(&mut self, realm: Realm, name: &'static str, length: u8, f: madcawl_js::Bound<madcawl_js::HostFn, Jsc, H>, data: u64) -> madcawl_js::Owned<Value> {
        let ctx = self.realm_ctx(realm);
        let fproto = self.helper(realm.0, Helper::FunctionProto);
        let kind = FnKind::Host { f: f.get(), data, thunk: thunk::<H> };
        let v = make_function(self, ctx, FnRec { kind, host_type: std::any::TypeId::of::<H>() }, fproto);
        let n = self.string_of(&madcawl_str::DomString::from_wtf8(name.as_bytes()));
        // stopped while naming it, the function stays usable, unnamed
        if self.call_helper(realm.0, Helper::Named, &[v, n, Value::number(f64::from(length))]).is_err() {
            self.after_stop();
        }
        self.unprotect(n);
        Jsc::owned(v)
    }

    pub(crate) fn helper(&self, realm: u32, h: Helper) -> Value {
        self.realms[realm as usize].helpers[h as usize]
    }

    /// Call helper `h` of `realm`: its result (uncounted), or the
    /// exception it threw.
    pub(crate) fn call_helper(&mut self, realm: u32, h: Helper, args: &[Value]) -> Result<Value, Value> {
        let ctx = self.realms[realm as usize].ctx;
        let f = self.helper(realm, h);
        let mut exc: JSValueRef = std::ptr::null();
        self.depth += 1;
        // SAFETY: a helper function of a live realm; `Value` is the word
        let r = unsafe { ffi::JSObjectCallAsFunction(ctx, f.0 as _, std::ptr::null_mut(), args.len(), args.as_ptr().cast(), &mut exc) };
        self.depth -= 1;
        if exc.is_null() { Ok(Value::from_ref(r)) } else { Err(Value::from_ref(exc)) }
    }

    /// The interface and prototype objects of `class` in `realm`,
    /// uncounted (the realm holds them).
    pub(crate) fn class_objects<H: Host<Jsc>>(&mut self, realm: u32, class: &'static ClassDef<Jsc, H>) -> (Value, Value) {
        if let Some(&p) = self.realms[realm as usize].protos.get(&class.id.0) {
            return p;
        }
        let ctx = self.realms[realm as usize].ctx;
        let (parent_iface, parent_proto) = match class.parent {
            Some(p) => self.class_objects(realm, p),
            None if class.error_prototype => (self.helper(realm, Helper::FunctionProto), self.helper(realm, Helper::ErrorProto)),
            None => (self.helper(realm, Helper::FunctionProto), self.helper(realm, Helper::ObjectProto)),
        };
        // SAFETY: a plain object of a live realm, whose parent is live
        let proto = unsafe {
            let p = Value(ffi::JSObjectMake(ctx, std::ptr::null_mut(), std::ptr::null_mut()) as u64);
            self.protect(p);
            ffi::JSObjectSetPrototype(ctx, p.0 as _, parent_proto.raw());
            p
        };
        let made = class.constructor.and_then(|c| self.constructor_object::<H>(realm, ctx, c.get(), proto));
        // SAFETY: a plain object of a live realm, with no record
        let iface = made.unwrap_or_else(|| Value(unsafe { ffi::JSObjectMake(ctx, self.classes.iface, std::ptr::null_mut()) } as u64));
        self.protect(iface);
        // SAFETY: both live objects of a live realm
        unsafe { ffi::JSObjectSetPrototype(ctx, iface.0 as _, parent_iface.raw()) };
        self.realms[realm as usize].protos.insert(class.id.0, (iface, proto));
        let name = self.string_of(&madcawl_str::DomString::from_wtf8(class.name.as_bytes()));
        self.define(realm, Helper::Interface, &[iface, proto, name, Value::number(f64::from(class.length))]);
        self.define_members(realm, ctx, class, iface, proto);
        self.unprotect(name);
        (iface, proto)
    }

    /// The interface object of a class whose constructor is `f` and
    /// prototype object `proto`: a bound script function around a host
    /// function calling `f`, uncounted (the caller protects it; the
    /// native function is held by the closure). A host function object
    /// gets no `new.target`, which a subclass needs; the bound function
    /// shows as native code. `None` when script was being stopped: the
    /// interface then throws on `new`, as one without a constructor.
    fn constructor_object<H: Host<Jsc>>(&mut self, realm: u32, ctx: JSContextRef, f: madcawl_js::HostFn, proto: Value) -> Option<Value> {
        let fproto = self.helper(realm, Helper::FunctionProto);
        let kind = FnKind::Host { f, data: 0, thunk: thunk::<H> };
        let native = make_function(self, ctx, FnRec { kind, host_type: std::any::TypeId::of::<H>() }, fproto);
        let made = self.call_helper(realm, Helper::Constructor, &[native, proto]);
        self.unprotect(native);
        if made.is_err() {
            self.after_stop();
        }
        made.ok()
    }

    fn define_members<H: Host<Jsc>>(&mut self, realm: u32, ctx: JSContextRef, class: &'static ClassDef<Jsc, H>, iface: Value, proto: Value) {
        let fproto = self.helper(realm, Helper::FunctionProto);
        let host_fn = |e: &Jsc, f: madcawl_js::HostFn| {
            let kind = FnKind::Host { f, data: 0, thunk: thunk::<H> };
            make_function(e, ctx, FnRec { kind, host_type: std::any::TypeId::of::<H>() }, fproto)
        };
        for a in class.attributes {
            let name = self.string_of(&madcawl_str::DomString::from_wtf8(a.name.as_bytes()));
            let g = host_fn(self, a.get.get());
            let s = a.set.map_or(UNDEFINED, |s| host_fn(self, s.get()));
            self.define(realm, Helper::Accessor, &[proto, name, g, s]);
            for v in [name, g, s] {
                self.unprotect(v);
            }
        }
        for o in class.operations {
            let name = self.string_of(&madcawl_str::DomString::from_wtf8(o.name.as_bytes()));
            let f = host_fn(self, o.call.get());
            self.define(realm, Helper::Method, &[proto, name, f, Value::number(f64::from(o.length))]);
            self.unprotect(name);
            self.unprotect(f);
        }
        for k in class.constants {
            let name = self.string_of(&madcawl_str::DomString::from_wtf8(k.name.as_bytes()));
            for target in [iface, proto] {
                self.define(realm, Helper::Constant, &[target, name, Value::number(k.value)]);
            }
            self.unprotect(name);
        }
    }

    /// Stopped midway (the watchdog ends helpers too), the class keeps
    /// the members defined so far; after an abort, later tasks see the
    /// class so.
    fn define(&mut self, realm: u32, h: Helper, args: &[Value]) {
        if self.call_helper(realm, h, args).is_err() {
            self.after_stop();
        }
    }
}

/// Let realm `r` go: its context and what only it holds.
pub(crate) fn detach(e: &mut Jsc, r: u32) {
    let d = &mut e.realms[r as usize];
    if d.detached {
        return;
    }
    d.detached = true;
    let mut held: Vec<Value> = d.protos.drain().flat_map(|(_, (i, p))| [i, p]).collect();
    held.append(&mut d.helpers);
    // what keeps the realm's wrappers: their records stay until finalized
    for rec in d.wrappers.drain() {
        // SAFETY: a record not yet freed
        let rec = unsafe { &mut *(rec as *mut crate::objects::WrapRec) };
        if std::mem::take(&mut rec.strong) {
            held.push(rec.js);
        }
        held.push(std::mem::replace(&mut rec.weak, Value(0)));
    }
    let ctx = d.ctx;
    for v in held {
        e.unprotect(v);
    }
    // SAFETY: made in make_realm; objects other realms hold stay in the group
    unsafe { ffi::JSGlobalContextRelease(ctx) };
    e.drop_pending_of(r);
    if let Some(live) = e.live_realm() {
        e.current = live;
    }
}
