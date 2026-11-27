//! Conversions of numbers, optional arguments with defaults and event
//! handler values, by Web IDL type; `convert` passes them on to the
//! generated glue.

use madcawl_js::{Engine, ErrorKind, ValueKind};
use madcawl_str::DomString;

use crate::convert::{Thrown, dom_string, throw};
use crate::host::DomHost;

/// A number as a Web IDL `long`: ECMAScript ToInt32, which wraps.
pub(crate) fn to_int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    n.trunc().rem_euclid(4_294_967_296.0) as u32 as i32
}

/// A Web IDL `unsigned long`: ECMAScript ToNumber, then modulo 2^32.
pub(crate) fn to_uint32<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<u32, Thrown> {
    let n = e.to_number(h, v).ok_or(Thrown)?;
    Ok(to_int32(n) as u32)
}

/// A Web IDL `long`: undefined converts to 0, an optional argument's
/// usual default.
pub(crate) fn to_long<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<i32, Thrown> {
    let n = e.to_number(h, v).ok_or(Thrown)?;
    Ok(to_int32(n))
}

/// A Web IDL `[EnforceRange] unsigned long long`: a TypeError for a
/// number that is not finite or, truncated, is outside 0 to 2^53 - 1.
pub(crate) fn enforce_u64<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<u64, Thrown> {
    let n = e.to_number(h, v).ok_or(Thrown)?;
    let x = n.trunc();
    if !n.is_finite() || !(0.0..=9_007_199_254_740_991.0).contains(&x) {
        return Err(throw(e, ErrorKind::Type, "the number is out of range"));
    }
    Ok(x as u64)
}

/// An `EventHandler` value: an object (called if callable when the
/// event comes), or null for anything else, as
/// `[LegacyTreatNonObjectAsNull]` converts it.
pub(crate) fn event_handler<E: Engine>(e: &mut E, v: E::Value) -> Option<E::Value> {
    matches!(e.kind(v), ValueKind::Object | ValueKind::Function).then_some(v)
}

/// An `any` argument whose default is null.
pub(crate) fn any_or_null<E: Engine>(e: &E, v: E::Value) -> E::Value {
    if e.kind(v) == ValueKind::Undefined { E::null() } else { v }
}

/// A `DOMString` argument with a default: `default` when undefined.
pub(crate) fn dom_string_or<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value, default: &[u8]) -> Result<DomString, Thrown> {
    if e.kind(v) == ValueKind::Undefined {
        return Ok(DomString::from_wtf8(default));
    }
    dom_string(e, h, v)
}

pub(crate) fn boolean_or<E: Engine>(e: &mut E, v: E::Value, default: bool) -> bool {
    if e.kind(v) == ValueKind::Undefined {
        default
    } else {
        e.to_boolean(v)
    }
}

/// A `[LegacyNullToEmptyString] DOMString` argument with a default:
/// `default` when undefined, the empty string when null.
pub(crate) fn dom_string_null_empty_or<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value, default: &[u8]) -> Result<DomString, Thrown> {
    if e.kind(v) == ValueKind::Undefined {
        return Ok(DomString::from_wtf8(default));
    }
    crate::convert::dom_string_null_empty(e, h, v)
}

/// A non-nullable `DOMString` result the implementation has none of:
/// the empty string.
pub(crate) fn or_empty(s: Option<DomString>) -> DomString {
    s.unwrap_or_else(DomString::empty)
}
