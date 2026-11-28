//! The helpers for buffer sources, records and host-made iterators each
//! realm makes once, after those of `window_helpers.rs`:
//! `Helper::ViewRange`, `Helper::OwnEntries` and `Helper::ArrayValues`,
//! in that order.

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
  ];
})()"#;
