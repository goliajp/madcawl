//! The helpers for buffer sources, records and host-made iterators each
//! realm makes once, after those of `window_helpers.rs`:
//! `Helper::ViewRange`, `Helper::OwnEntries`, `Helper::ArrayValues`,
//! `Helper::Clone` and `Helper::Json`, in that order.

// Taken before any page script, as in `HELPERS`.
pub(crate) const DATA_HELPERS: &str = r#"(function () {
  'use strict';
  const dp = Object.defineProperty;
  const apply = Reflect.apply;
  const U8 = Uint8Array;
  const isView = ArrayBuffer.isView;
  const DV = DataView.prototype;
  const gopd = Reflect.getOwnPropertyDescriptor;
  const getter = (p, k) => gopd(p, k).get;
  const dvBuffer = getter(DV, 'buffer');
  const dvOffset = getter(DV, 'byteOffset');
  const dvLength = getter(DV, 'byteLength');
  const values = Array.prototype.values;
  const parse = JSON.parse;
  const isArray = Array.isArray;
  const A = Array;
  const M = Map;
  const S = Set;
  const D = Date;
  const RE = RegExp;
  const DVC = DataView;
  const mapGet = M.prototype.get;
  const mapSet = M.prototype.set;
  const mapEach = M.prototype.forEach;
  const mapSize = getter(M.prototype, 'size');
  const setAdd = S.prototype.add;
  const setEach = S.prototype.forEach;
  const setSize = getter(S.prototype, 'size');
  const getTime = D.prototype.getTime;
  const reSource = getter(RE.prototype, 'source');
  const reFlags = getter(RE.prototype, 'flags');
  const abLength = getter(ArrayBuffer.prototype, 'byteLength');
  const TA = Reflect.getPrototypeOf(U8.prototype);
  const taTag = getter(TA, Symbol.toStringTag);
  const taBuffer = getter(TA, 'buffer');
  const typed = { __proto__: null, Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array, Int32Array, Uint32Array,
    Float32Array, Float64Array, BigInt64Array, BigUint64Array };
  const isError = Error.isError;
  const errors = { __proto__: null, Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError };
  // whether `f` applies to `x`, a brand check by a builtin's own getter or method
  const is = (f, x) => {
    try {
      apply(f, x, []);
      return true;
    } catch (_) {
      return false;
    }
  };
  const R = { __proto__: null, get: Reflect.get, ownKeys: Reflect.ownKeys, gopd };
  const put = (a, i, v) => { dp(a, i, ({ __proto__: null, value: v, writable: true, enumerable: true, configurable: true })); };
  return [
    // a Uint8Array over the bytes `v`, a typed array or a DataView,
    // views, empty once its buffer is detached; undefined for any other
    // value. The caller asks only for what is no typed array
    (v) => {
      if (!isView(v)) return undefined;
      const b = apply(dvBuffer, v, []);
      let offset, length;
      try {
        offset = apply(dvOffset, v, []);
        length = apply(dvLength, v, []);
      } catch (_) {
        return new U8(0);
      }
      return new U8(b, offset, length);
    },
    // the own enumerable string-keyed properties of `o`, keys and values
    // alternating, each value converted by ToString as it is read
    (o) => {
      const keys = R.ownKeys(o);
      const out = [];
      let n = 0;
      for (let i = 0; i < keys.length; i++) {
        const k = keys[i];
        if (typeof k !== 'string') continue;
        const d = R.gopd(o, k);
        if (d === undefined || !d.enumerable) continue;
        put(out, n++, k);
        put(out, n++, `${R.get(o, k)}`);
      }
      return out;
    },
    // an Array Iterator over `a`, an array the host made
    (a) => apply(values, a, []),
    // the structured clone of `v` made in this realm, in an array of one;
    // undefined when it holds a function, a symbol or an object
    // `platform` says is a platform object
    (v, platform) => {
      const memory = new M();
      let bad = false;
      const bytes = (b) => apply(taBuffer, new U8(new U8(b)), []);
      const pairs = (each, x) => {
        const out = [];
        let n = 0;
        apply(each, x, [(a, b) => { put(out, n++, b); put(out, n++, a); }]);
        return out;
      };
      const own = (x, out) => {
        const keys = R.ownKeys(x);
        for (let i = 0; i < keys.length && !bad; i++) {
          const k = keys[i];
          if (typeof k !== 'string') continue;
          const d = R.gopd(x, k);
          if (d !== undefined && d.enumerable) put(out, k, clone(R.get(x, k)));
        }
      };
      const clone = (x) => {
        const t = typeof x;
        if (t === 'symbol' || t === 'function') {
          bad = true;
          return undefined;
        }
        if (x === null || t !== 'object') return x;
        const seen = apply(mapGet, memory, [x]);
        if (seen !== undefined) return seen;
        const tag = apply(taTag, x, []);
        let out;
        let fill = null;
        if (tag !== undefined) out = new typed[tag](x);
        else if (isArray(x)) [out, fill] = [new A(R.get(x, 'length')), own];
        else if (is(abLength, x)) out = bytes(x);
        else if (is(dvBuffer, x)) out = new DVC(bytes(apply(dvBuffer, x, [])), apply(dvOffset, x, []), apply(dvLength, x, []));
        else if (is(getTime, x)) out = new D(apply(getTime, x, []));
        else if (is(reSource, x)) out = new RE(apply(reSource, x, []), apply(reFlags, x, []));
        else if (is(mapSize, x)) [out, fill] = [new M(), (x, o) => { const p = pairs(mapEach, x); for (let i = 0; i < p.length && !bad; i += 2) apply(mapSet, o, [clone(p[i]), clone(p[i + 1])]); }];
        else if (is(setSize, x)) [out, fill] = [new S(), (x, o) => { const p = pairs(setEach, x); for (let i = 0; i < p.length && !bad; i += 2) apply(setAdd, o, [clone(p[i])]); }];
        else if (isError(x)) {
          const name = R.get(x, 'name');
          const message = R.get(x, 'message');
          out = new (errors[name] || errors.Error)(message === undefined ? undefined : `${message}`);
        } else if (apply(platform, undefined, [x])) {
          bad = true;
          return undefined;
        } else [out, fill] = [{}, own];
        apply(mapSet, memory, [x, out]);
        if (fill) fill(x, out);
        return out;
      };
      const r = clone(v);
      return bad ? undefined : [r];
    },
    // JSON text `s` parsed, in an array of one; undefined when it is no JSON
    (s) => {
      try {
        return [apply(parse, undefined, [s])];
      } catch (_) {
        return undefined;
      }
    },
  ];
})()"#;
