//! Conversions between script values and the types the
//! implementations take, by Web IDL type; the generated glue calls
//! these.

use madcawl_dom::NodeId;
use madcawl_js::{ClassDef, Engine, ErrorKind, Owned, ValueKind};
use madcawl_str::{Atom, DomString};

use crate::host::DomHost;
use crate::side::SideRef;

/// An exception is registered in the engine.
#[derive(Debug)]
pub struct Thrown;

pub(crate) fn throw<E: Engine>(e: &mut E, kind: ErrorKind, message: &str) -> Thrown {
    let _sentinel = e.throw_error(kind, message).into_raw();
    Thrown
}

/// Throw a `DOMException` named `name`. The host makes the object: a
/// plain one with the members script reads (`name`, `message`, `code`)
/// until the interface is bound. The members go in by [[Set]], which a
/// page's setter on Object.prototype can intercept or refuse; that is
/// accepted until the interface is bound and they are defined instead.
pub(crate) fn throw_dom<E: Engine>(e: &mut E, h: &mut DomHost<E>, name: &'static str, message: &str) -> Thrown {
    let x = crate::impls::dom_exception::DomException {
        name: DomString::from_wtf8(name.as_bytes()),
        message: DomString::from_wtf8(message.as_bytes()),
    };
    let r = h.sides.add(crate::side::SideData::DomException(x));
    let ex = h.wrap_side(e, r);
    let _sentinel = e.throw(ex).into_raw();
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

impl<E: Engine> IntoJs<E> for u32 {
    fn into_js(self, _: &mut E, _: &mut DomHost<E>) -> Owned<E::Value> {
        plain::<E>(E::number(f64::from(self)))
    }
}

impl<E: Engine> IntoJs<E> for f64 {
    fn into_js(self, _: &mut E, _: &mut DomHost<E>) -> Owned<E::Value> {
        plain::<E>(E::number(self))
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

/// An array of `items`, made from the realm's own `Array` without
/// running script; their counts are given back.
pub(crate) fn array_of<E: Engine>(e: &mut E, h: &mut DomHost<E>, items: Vec<Owned<E::Value>>) -> Owned<E::Value> {
    let values: Vec<E::Value> = items.iter().map(Owned::get).collect();
    let r = e.new_array(h.realm, &values);
    for v in items {
        e.release(v);
    }
    r
}

pub(crate) fn run<E: Engine, T: IntoJs<E>>(
    e: &mut E,
    h: &mut DomHost<E>,
    f: impl FnOnce(&mut E, &mut DomHost<E>) -> Result<T, Thrown>,
) -> Owned<E::Value> {
    let r = f(e, h);
    // before script runs again: its collector may run at any allocation
    h.regroup(e);
    match r {
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

/// A number as a Web IDL `long`: ECMAScript ToInt32, which wraps.
pub(crate) fn to_int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    n.trunc().rem_euclid(4_294_967_296.0) as u32 as i32
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

/// `this` as an event target: a node, or a side object of `class`.
pub(crate) fn this_target<E: Engine>(
    e: &mut E,
    h: &DomHost<E>,
    this: E::Value,
    class: &'static ClassDef<E, DomHost<E>>,
) -> Result<crate::event::Target, Thrown> {
    match e.host_of(this, class) {
        Some(obj) => Ok(match h.sides.of(obj) {
            Some(r) => crate::event::Target::Side(r),
            None => crate::event::Target::Node(h.node(obj)),
        }),
        None => Err(throw(e, ErrorKind::Type, "illegal invocation")),
    }
}

/// `this` as a side object of `class`.
pub(crate) fn this_side<E: Engine>(
    e: &mut E,
    h: &DomHost<E>,
    this: E::Value,
    class: &'static ClassDef<E, DomHost<E>>,
) -> Result<SideRef, Thrown> {
    match e.host_of(this, class).and_then(|obj| h.sides.of(obj)) {
        Some(r) => Ok(r),
        None => Err(throw(e, ErrorKind::Type, "illegal invocation")),
    }
}

pub(crate) fn side<E: Engine>(
    e: &mut E,
    h: &DomHost<E>,
    v: E::Value,
    class: &'static ClassDef<E, DomHost<E>>,
) -> Result<SideRef, Thrown> {
    match e.host_of(v, class).and_then(|obj| h.sides.of(obj)) {
        Some(r) => Ok(r),
        None => Err(throw(e, ErrorKind::Type, "the argument is not an object of that interface")),
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

/// A `DOMString` that is a name, as the atom that spells it, held by the
/// document's holder. A string value gives its atom with no block made;
/// any other value is converted by ToString first.
pub(crate) fn atom<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<Atom, Thrown> {
    if let Some(a) = e.to_atom(v, &h.names) {
        return Ok(a);
    }
    let s = dom_string(e, h, v)?;
    let s = e.string(s);
    let a = e.to_atom(s.get(), &h.names).expect("a string value");
    e.release(s);
    Ok(a)
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

/// A Web IDL `unsigned long`: ECMAScript ToNumber, then modulo 2^32.
pub(crate) fn to_uint32<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<u32, Thrown> {
    let n = e.to_number(h, v).ok_or(Thrown)?;
    Ok(to_int32(n) as u32)
}

/// A `[LegacyNullToEmptyString] DOMString`: null is the empty string.
pub(crate) fn dom_string_null_empty<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<DomString, Thrown> {
    if e.kind(v) == ValueKind::Null {
        return Ok(DomString::empty());
    }
    dom_string(e, h, v)
}

/// An `any` argument whose default is null.
pub(crate) fn any_or_null<E: Engine>(e: &E, v: E::Value) -> E::Value {
    if e.kind(v) == ValueKind::Undefined { E::null() } else { v }
}

/// A script value an `any` member returns, with its count.
pub(crate) struct Any<E: Engine>(pub Owned<E::Value>);

impl<E: Engine> IntoJs<E> for Any<E> {
    fn into_js(self, _: &mut E, _: &mut DomHost<E>) -> Owned<E::Value> {
        self.0
    }
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
