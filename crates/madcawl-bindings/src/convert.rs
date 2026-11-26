//! Conversions between script values and the types the
//! implementations take, by Web IDL type; the generated glue calls
//! these.

use madcawl_dom::NodeId;
use madcawl_js::{ClassDef, Engine, ErrorKind, Key, Owned, ValueKind};
use madcawl_str::{Atom, DomString};

use crate::host::DomHost;

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
    let ex = e.new_object(h.realm);
    let code = legacy_code(name) as f64;
    let name = e.string(DomString::from_wtf8(name.as_bytes()));
    let message = e.string(DomString::from_wtf8(message.as_bytes()));
    for (key, v) in [("name", name.get()), ("message", message.get()), ("code", E::number(code))] {
        let _ = e.set(h, ex.get(), Key::Str(&DomString::from_wtf8(key.as_bytes())), v);
    }
    e.release(name);
    e.release(message);
    let _sentinel = e.throw(ex).into_raw();
    Thrown
}

/// The legacy `code` of the exception names thrown so far; 0 for the
/// names that have none.
fn legacy_code(name: &str) -> u16 {
    match name {
        "HierarchyRequestError" => 3,
        "InvalidCharacterError" => 5,
        "NotFoundError" => 8,
        "SyntaxError" => 12,
        "QuotaExceededError" => 22,
        _ => 0,
    }
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

/// A list made when asked for, as an array of wrappers: what an
/// operation returns until `NodeList` and `HTMLCollection` are bound.
pub(crate) struct StaticList(pub Vec<NodeId>);

impl<E: Engine> IntoJs<E> for StaticList {
    fn into_js(self, e: &mut E, h: &mut DomHost<E>) -> Owned<E::Value> {
        let items: Vec<Owned<E::Value>> = self.0.into_iter().map(|n| h.wrap(e, n)).collect();
        let global = e.global(h.realm);
        let array = e.get(h, global.get(), Key::Str(&DomString::from_wtf8(b"Array")));
        let of = e.get(h, array.get(), Key::Str(&DomString::from_wtf8(b"of")));
        let values: Vec<E::Value> = items.iter().map(Owned::get).collect();
        let r = e.call(h, of.get(), array.get(), &values);
        for v in items.into_iter().chain([of, array, global]) {
            e.release(v);
        }
        r
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
