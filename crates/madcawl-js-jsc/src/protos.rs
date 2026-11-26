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
        let helpers: Vec<Value> = (0..=Helper::DerefAll as u32)
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
            None => (self.helper(realm, Helper::FunctionProto), self.helper(realm, Helper::ObjectProto)),
        };
        let ctor = class.constructor.map_or(std::ptr::null_mut(), |c| {
            let kind = FnKind::Host { f: c.get(), data: 0, thunk: thunk::<H> };
            Box::into_raw(Box::new(FnRec { kind, host_type: std::any::TypeId::of::<H>() })).cast()
        });
        // SAFETY: plain objects of a live realm; the record is freed by the
        // interface class's finalizer
        let (iface, proto) = unsafe {
            let i = Value(ffi::JSObjectMake(ctx, self.classes.iface, ctor) as u64);
            let p = Value(ffi::JSObjectMake(ctx, std::ptr::null_mut(), std::ptr::null_mut()) as u64);
            self.protect(i);
            self.protect(p);
            ffi::JSObjectSetPrototype(ctx, i.0 as _, parent_iface.raw());
            ffi::JSObjectSetPrototype(ctx, p.0 as _, parent_proto.raw());
            (i, p)
        };
        self.realms[realm as usize].protos.insert(class.id.0, (iface, proto));
        let name = self.string_of(&madcawl_str::DomString::from_wtf8(class.name.as_bytes()));
        self.define(realm, Helper::Interface, &[iface, proto, name]);
        self.define_members(realm, ctx, class, iface, proto);
        self.unprotect(name);
        (iface, proto)
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
