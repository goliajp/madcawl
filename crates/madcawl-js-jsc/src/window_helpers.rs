//! The helpers for windows and event handlers each realm makes once,
//! after those of `helpers.rs`: `Helper::Proxy` to `Helper::CrossOrigin`,
//! in that order.

// Taken before any page script, as in `HELPERS`. The one exception is
// the direct `eval` that compiles a handler: it is looked up when
// called, through the handler's scopes, so a page that replaced it runs
// its own code there.
pub(crate) const WINDOW_HELPERS: &str = r#"(function () {
  'use strict';
  const dp = Object.defineProperty;
  const create = Object.create;
  const freeze = Object.freeze;
  const P = Proxy;
  const R = { __proto__: null, get: Reflect.get, set: Reflect.set, ownKeys: Reflect.ownKeys,
    gopd: Reflect.getOwnPropertyDescriptor, define: Reflect.defineProperty, proto: Reflect.getPrototypeOf,
    del: Reflect.deleteProperty };
  // the standard's CrossOriginPropertyFallback keys
  const quiet = ['then', Symbol.toStringTag, Symbol.hasInstance, Symbol.isConcatSpreadable];
  const fallback = (k) => k === quiet[0] || k === quiet[1] || k === quiet[2] || k === quiet[3];
  // the scopes come innermost first (element, form, document). Sloppy,
  // for `with`, and made by `Function`, so that the code it compiles
  // sees the global scope and the scopes, not the names in here
  const handler = Function('__mcParams', '__mcBody', '__mcScopes', `
    const __mcS = [{ __proto__: null }, { __proto__: null }, { __proto__: null }];
    for (let __mcI = 0; __mcI < 3 && __mcI < __mcScopes.length; __mcI++) __mcS[2 - __mcI] = __mcScopes[__mcI];
    let __mcP = '';
    for (let __mcI = 0; __mcI < __mcParams.length; __mcI++) __mcP += (__mcI ? ',' : '') + __mcParams[__mcI];
    with (__mcS[0]) with (__mcS[1]) with (__mcS[2]) {
      return eval('(function (' + __mcP + ') {\\n' + __mcBody + '\\n})');
    }`);
  return [
    // a WindowProxy, and the record whose `g` is its target: the host
    // sets `g` as a plain property, which runs no script
    (t) => {
      const state = { __proto__: null, g: t };
      const h = {
        __proto__: null,
        get: (_, k) => R.get(state.g, k),
        set: (_, k, v) => R.set(state.g, k, v),
        has: (_, k) => k in state.g,
        deleteProperty: (_, k) => R.del(state.g, k),
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
    (f, n, l) => {
      dp(f, 'name', ({ __proto__: null, value: n, configurable: true }));
      dp(f, 'length', ({ __proto__: null, value: l, configurable: true }));
    },
    // a window of another origin: `t`'s own members, frozen with their
    // functions, so that no caller leaves anything there for another;
    // the fallback keys, own, configurable and undefined; `deny(k)`,
    // which throws, for the rest
    (t, deny) => {
      const keys = R.ownKeys(t);
      for (let i = 0; i < keys.length; i++) {
        const d = R.gopd(t, keys[i]);
        freeze(d.get !== undefined ? d.get : d.value);
      }
      freeze(t);
      const own = (k) => R.gopd(t, k);
      const quietDesc = () => ({ __proto__: null, value: undefined, writable: false, enumerable: false, configurable: true });
      return new P(create(null), {
        __proto__: null,
        get: (_, k) => (own(k) ? R.get(t, k) : fallback(k) ? undefined : deny(k)),
        set: (_, k) => deny(k),
        has: (_, k) => own(k) !== undefined || fallback(k) || deny(k),
        getOwnPropertyDescriptor: (_, k) => {
          const d = own(k);
          if (d === undefined) return fallback(k) ? quietDesc() : deny(k);
          d.configurable = true;
          return d;
        },
        defineProperty: (_, k) => deny(k),
        deleteProperty: (_, k) => deny(k),
        ownKeys: () => {
          const out = R.ownKeys(t);
          for (let i = 0; i < quiet.length; i++) {
            dp(out, out.length, ({ __proto__: null, value: quiet[i], writable: true, enumerable: true, configurable: true }));
          }
          return out;
        },
        getPrototypeOf: () => null,
        setPrototypeOf: (_, p) => p === null,
        isExtensible: () => true,
        preventExtensions: () => false,
      });
    },
  ];
})()"#;
