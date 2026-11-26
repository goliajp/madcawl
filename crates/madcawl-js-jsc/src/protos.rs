//! Realms: each a global context of the isolate's group, with a few
//! script helpers made once (defining accessors, making errors, queueing
//! microtasks, weak references, the WindowProxy, handler compilation)
//! and the interface and prototype objects of the host classes.

use std::collections::HashMap;

use madcawl_js::{ClassDef, Host};

use crate::ffi::{self, JSContextRef, JSGlobalContextRef, JSValueRef};
use crate::isolate::{Jsc, Realm};
use crate::objects::{FnKind, FnRec, make_function, thunk};
use crate::value::{UNDEFINED, Value};

/// The helpers, in the order the script below returns them.
#[derive(Clone, Copy)]
pub(crate) enum Helper {
    Accessor = 0,
    Method = 1,
    Constant = 2,
    Interface = 3,
    Error = 4,
    EnqueueAll = 5,
    WeakRef = 6,
    Deref = 7,
    Proxy = 8,
    Handler = 9,
    Named = 10,
    FunctionProto = 11,
    ObjectProto = 12,
}

const HELPERS: &str = r#"(function () {
  const dp = Object.defineProperty;
  const named = (f, n, l) => {
    dp(f, 'name', { value: n, configurable: true });
    dp(f, 'length', { value: l, configurable: true });
  };
  const resolved = Promise.resolve();
  // the scopes come innermost first (element, form, document)
  function handler(__mcParams, __mcBody, __mcScopes) {
    const __mcS = __mcScopes.slice(0, 3).reverse();
    while (__mcS.length < 3) __mcS.unshift(Object.create(null));
    with (__mcS[0]) with (__mcS[1]) with (__mcS[2]) {
      return eval('(function (' + __mcParams.join(',') + ') {\n' + __mcBody + '\n})');
    }
  }
  return [
    (o, n, g, s) => {
      named(g, 'get ' + n, 0);
      if (s) named(s, 'set ' + n, 1);
      dp(o, n, { get: g, set: s, enumerable: true, configurable: true });
    },
    (o, n, f, l) => {
      named(f, n, l);
      dp(o, n, { value: f, writable: true, enumerable: true, configurable: true });
    },
    (o, n, v) => { dp(o, n, { value: v, enumerable: true }); },
    (i, p, n) => {
      dp(i, 'prototype', { value: p });
      dp(p, 'constructor', { value: i, writable: true, configurable: true });
      named(i, n, 0);
      dp(p, Symbol.toStringTag, { value: n, configurable: true });
    },
    (k, m) => new [TypeError, RangeError, SyntaxError, ReferenceError][k](m),
    (jobs) => { for (const f of jobs) resolved.then(f); },
    (o) => new WeakRef(o),
    (w) => w.deref(),
    (t) => {
      let g = t;
      const h = {
        get: (_, k) => Reflect.get(g, k),
        set: (_, k, v) => Reflect.set(g, k, v),
        has: (_, k) => k in g,
        deleteProperty: (_, k) => delete g[k],
        ownKeys: () => Reflect.ownKeys(g),
        getOwnPropertyDescriptor: (_, k) => {
          const d = Reflect.getOwnPropertyDescriptor(g, k);
          if (d) d.configurable = true;
          return d;
        },
        defineProperty: (_, k, d) => Reflect.defineProperty(g, k, d),
        getPrototypeOf: () => Reflect.getPrototypeOf(g),
      };
      return [new Proxy({}, h), (n) => { g = n; }];
    },
    handler,
    named,
    Function.prototype,
    Object.prototype,
  ];
})()"#;

pub(crate) struct RealmData {
    pub ctx: JSGlobalContextRef,
    pub detached: bool,
    /// Each host class's interface object and prototype object, by
    /// class id, made when first asked for; counted.
    pub protos: HashMap<u16, (Value, Value)>,
    pub helpers: Vec<Value>,
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
        let helpers = (0..=Helper::ObjectProto as u32)
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
        self.realms.push(RealmData { ctx, detached: false, protos: HashMap::new(), helpers });
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
        // SAFETY: plain objects of a live realm
        let (iface, proto) = unsafe {
            let i = Value(ffi::JSObjectMake(ctx, self.classes.iface, std::ptr::null_mut()) as u64);
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

    fn define(&mut self, realm: u32, h: Helper, args: &[Value]) {
        assert!(self.call_helper(realm, h, args).is_ok(), "defining a host class member");
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
    let ctx = d.ctx;
    for v in held {
        e.unprotect(v);
    }
    // SAFETY: made in make_realm; objects other realms hold stay in the group
    unsafe { ffi::JSGlobalContextRelease(ctx) };
}
