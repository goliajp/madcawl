//! Conversions between script values and the types the
//! implementations take, by Web IDL type; the generated glue calls
//! these.

use madcawl_dom::NodeId;
use madcawl_js::{ClassDef, Engine, ErrorKind, Owned, ValueKind};
use madcawl_str::DomString;

use crate::host::DomHost;

/// An exception is registered in the engine.
#[derive(Debug)]
pub struct Thrown;

pub(crate) fn throw<E: Engine>(e: &mut E, kind: ErrorKind, message: &str) -> Thrown {
    let _sentinel = e.throw_error(kind, message).into_raw();
    Thrown
}

pub(crate) trait IntoJs<E: Engine> {
    fn into_js(self, e: &mut E, h: &mut DomHost<E>) -> Owned<E::Value>;
}

fn plain<E: Engine>(v: E::Value) -> Owned<E::Value> {
    // SAFETY: undefined, null, booleans and numbers carry no count
    unsafe { Owned::from_raw(v) }
}

impl<E: Engine> IntoJs<E> for () {
    fn into_js(self, _: &mut E, _: &mut DomHost<E>) -> Owned<E::Value> {
        plain::<E>(E::undefined())
    }
}

impl<E: Engine> IntoJs<E> for bool {
    fn into_js(self, _: &mut E, _: &mut DomHost<E>) -> Owned<E::Value> {
        plain::<E>(E::boolean(self))
    }
}

impl<E: Engine> IntoJs<E> for u16 {
    fn into_js(self, _: &mut E, _: &mut DomHost<E>) -> Owned<E::Value> {
        plain::<E>(E::number(self as f64))
    }
}

impl<E: Engine> IntoJs<E> for DomString {
    fn into_js(self, e: &mut E, _: &mut DomHost<E>) -> Owned<E::Value> {
        e.string(self)
    }
}

impl<E: Engine> IntoJs<E> for NodeId {
    fn into_js(self, e: &mut E, h: &mut DomHost<E>) -> Owned<E::Value> {
        h.wrap(e, self)
    }
}

impl<E: Engine, T: IntoJs<E>> IntoJs<E> for Option<T> {
    fn into_js(self, e: &mut E, h: &mut DomHost<E>) -> Owned<E::Value> {
        match self {
            Some(x) => x.into_js(e, h),
            None => plain::<E>(E::null()),
        }
    }
}

pub(crate) fn run<E: Engine, T: IntoJs<E>>(
    e: &mut E,
    h: &mut DomHost<E>,
    f: impl FnOnce(&mut E, &mut DomHost<E>) -> Result<T, Thrown>,
) -> Owned<E::Value> {
    match f(e, h) {
        Ok(v) => v.into_js(e, h),
        Err(Thrown) => E::thrown(),
    }
}

pub(crate) fn need<E: Engine>(e: &mut E, args: &[E::Value], n: usize) -> Result<(), Thrown> {
    if args.len() < n {
        return Err(throw(e, ErrorKind::Type, "not enough arguments"));
    }
    Ok(())
}

pub(crate) fn arg<E: Engine>(args: &[E::Value], i: usize) -> E::Value {
    args.get(i).copied().unwrap_or_else(E::undefined)
}

pub(crate) fn this_node<E: Engine>(
    e: &mut E,
    h: &DomHost<E>,
    this: E::Value,
    class: &'static ClassDef<E, DomHost<E>>,
) -> Result<NodeId, Thrown> {
    match e.host_of(this, class) {
        Some(obj) => Ok(h.node(obj)),
        None => Err(throw(e, ErrorKind::Type, "illegal invocation")),
    }
}

pub(crate) fn node<E: Engine>(
    e: &mut E,
    h: &DomHost<E>,
    v: E::Value,
    class: &'static ClassDef<E, DomHost<E>>,
) -> Result<NodeId, Thrown> {
    match e.host_of(v, class) {
        Some(obj) => Ok(h.node(obj)),
        None => Err(throw(e, ErrorKind::Type, "the argument is not a node of that interface")),
    }
}

pub(crate) fn node_or_null<E: Engine>(
    e: &mut E,
    h: &DomHost<E>,
    v: E::Value,
    class: &'static ClassDef<E, DomHost<E>>,
) -> Result<Option<NodeId>, Thrown> {
    if matches!(e.kind(v), ValueKind::Undefined | ValueKind::Null) {
        return Ok(None);
    }
    node(e, h, v, class).map(Some)
}

pub(crate) fn dom_string<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    v: E::Value,
) -> Result<DomString, Thrown> {
    e.to_string(h, v).ok_or(Thrown)
}

pub(crate) fn dom_string_or_null<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    v: E::Value,
) -> Result<Option<DomString>, Thrown> {
    if matches!(e.kind(v), ValueKind::Undefined | ValueKind::Null) {
        return Ok(None);
    }
    dom_string(e, h, v).map(Some)
}

pub(crate) fn boolean_or<E: Engine>(e: &mut E, v: E::Value, default: bool) -> bool {
    if e.kind(v) == ValueKind::Undefined {
        default
    } else {
        e.to_boolean(v)
    }
}

/// A callback interface value: a function, or an object whose
/// `handleEvent` is called (not yet: such an object is stored, and
/// calling it throws).
pub(crate) fn callback_or_null<E: Engine>(e: &mut E, v: E::Value) -> Result<Option<E::Value>, Thrown> {
    match e.kind(v) {
        ValueKind::Undefined | ValueKind::Null => Ok(None),
        ValueKind::Object | ValueKind::Function => Ok(Some(v)),
        _ => Err(throw(e, ErrorKind::Type, "the listener is not an object")),
    }
}
