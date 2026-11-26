//! The script helpers each realm makes once, and their order.

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
    Call = 13,
    Group = 14,
    Ungroup = 15,
    DerefAll = 16,
    Constructor = 17,
}

impl Helper {
    pub(crate) const LAST: Helper = Helper::Constructor;
}

// Page script runs in the realm and may replace any global or
// prototype member: everything the helpers use is taken when the realm
// is made, before any page script, and called through `apply`; their
// descriptors and arrays have no prototype to intercept a lookup. The
// one exception is the direct `eval` that compiles a handler: it is
// looked up when called, through the handler's scopes, so a page that
// replaced it runs its own code there.
pub(crate) const HELPERS: &str = r#"(function () {
  const dp = Object.defineProperty;
  const apply = Reflect.apply;
  const create = Object.create;
  const put = (a, i, v) => { dp(a, i, ({ __proto__: null, value: v, writable: true, enumerable: true, configurable: true })); };
  const named = (f, n, l) => {
    dp(f, 'name', ({ __proto__: null, value: n, configurable: true }));
    dp(f, 'length', ({ __proto__: null, value: l, configurable: true }));
  };
  const errors = [TypeError, RangeError, SyntaxError, ReferenceError];
  const then = Promise.prototype.then;
  const resolved = Promise.resolve();
  // `then` reads the promise's constructor for its species: an own
  // undefined one gives the original Promise whatever the page did
  dp(resolved, 'constructor', ({ __proto__: null, value: undefined }));
  const WR = WeakRef;
  const deref = WR.prototype.deref;
  const S = Set;
  const setAdd = S.prototype.add;
  const setDelete = S.prototype.delete;
  const WM = WeakMap;
  const wmGet = WM.prototype.get;
  const wmSet = WM.prototype.set;
  const wmDelete = WM.prototype.delete;
  const P = Proxy;
  const R = { __proto__: null, get: Reflect.get, set: Reflect.set, ownKeys: Reflect.ownKeys,
    gopd: Reflect.getOwnPropertyDescriptor, define: Reflect.defineProperty, proto: Reflect.getPrototypeOf,
    setProto: Reflect.setPrototypeOf };
  // each placed wrapper to its tree's group, which holds every wrapper
  // of the tree: one held keeps all, and none held lets all go
  const groupOf = new WM();
  const join = (g, w) => {
    const old = apply(wmGet, groupOf, [w]);
    if (old === g) return;
    if (old) apply(setDelete, old, [w]);
    apply(setAdd, g, [w]);
    apply(wmSet, groupOf, [w, g]);
  };
  // the scopes come innermost first (element, form, document)
  function handler(__mcParams, __mcBody, __mcScopes) {
    const __mcS = [create(null), create(null), create(null)];
    for (let i = 0; i < 3 && i < __mcScopes.length; i++) __mcS[2 - i] = __mcScopes[i];
    let __mcP = '';
    for (let i = 0; i < __mcParams.length; i++) __mcP += (i ? ',' : '') + __mcParams[i];
    with (__mcS[0]) with (__mcS[1]) with (__mcS[2]) {
      return eval('(function (' + __mcP + ') {\n' + __mcBody + '\n})');
    }
  }
  return [
    (o, n, g, s) => {
      named(g, 'get ' + n, 0);
      if (s) named(s, 'set ' + n, 1);
      dp(o, n, ({ __proto__: null, get: g, set: s, enumerable: true, configurable: true }));
    },
    (o, n, f, l) => {
      named(f, n, l);
      dp(o, n, ({ __proto__: null, value: f, writable: true, enumerable: true, configurable: true }));
    },
    (o, n, v) => { dp(o, n, ({ __proto__: null, value: v, enumerable: true })); },
    (i, p, n) => {
      dp(i, 'prototype', ({ __proto__: null, value: p, writable: false }));
      dp(p, 'constructor', ({ __proto__: null, value: i, writable: true, configurable: true }));
      named(i, n, 0);
      dp(p, Symbol.toStringTag, ({ __proto__: null, value: n, configurable: true }));
    },
    (k, m) => new errors[k](m),
    (jobs) => { for (let i = 0; i < jobs.length; i++) apply(then, resolved, [jobs[i]]); },
    (o) => new WR(o),
    (w) => apply(deref, w, []),
    // a WindowProxy, and the record whose `g` is its target: the host
    // sets `g` as a plain property, which runs no script
    (t) => {
      const state = { __proto__: null, g: t };
      const h = {
        __proto__: null,
        get: (_, k) => R.get(state.g, k),
        set: (_, k, v) => R.set(state.g, k, v),
        has: (_, k) => k in state.g,
        deleteProperty: (_, k) => delete state.g[k],
        ownKeys: () => R.ownKeys(state.g),
        getOwnPropertyDescriptor: (_, k) => {
          const d = R.gopd(state.g, k);
          if (d) d.configurable = true;
          return d;
        },
        defineProperty: (_, k, d) => R.define(state.g, k, d),
        getPrototypeOf: () => R.proto(state.g),
      };
      return [new P(create(null), h), state];
    },
    handler,
    named,
    Function.prototype,
    Object.prototype,
    Function.prototype.call,
    // the group of `r`, or a new one when `r` is undefined; a `WeakRef`
    // for each wrapper whose `need` is true
    (r, ws, need) => {
      let g;
      if (r === undefined) {
        g = new S();
      } else {
        g = apply(wmGet, groupOf, [r]);
        if (!g) {
          g = new S();
          join(g, r);
        }
      }
      const out = create(null);
      let n = 0;
      for (let i = 0; i < ws.length; i++) {
        join(g, ws[i]);
        if (need[i]) put(out, n++, new WR(ws[i]));
      }
      return out;
    },
    (ws) => {
      for (let i = 0; i < ws.length; i++) {
        const w = ws[i];
        const g = apply(wmGet, groupOf, [w]);
        if (g) {
          apply(setDelete, g, [w]);
          apply(wmDelete, groupOf, [w]);
        }
      }
    },
    (rs) => {
      const out = create(null);
      for (let i = 0; i < rs.length; i++) put(out, i, apply(deref, rs[i], []));
      return out;
    },
    // the interface object of a class with a constructor: `new` calls
    // the host's constructor and gives the object the prototype that
    // `new.target` names, so a subclass gets its own (Web IDL takes
    // the interface's when that is not an object); a call throws
    (native) => function (...args) {
      if (new.target === undefined) throw new errors[0]('Illegal constructor');
      const o = apply(native, undefined, args);
      const p = new.target.prototype;
      if (p !== null && (typeof p === 'object' || typeof p === 'function') && R.proto(o) !== p) R.setProto(o, p);
      return o;
    },
  ];
})()"#;
