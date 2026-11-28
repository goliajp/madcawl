//! Web IDL conversions the generated glue leaves to the implementations,
//! which take such arguments as `any`: dictionary members, and sequences
//! read from an iterable by the iterator protocol.

use madcawl_js::{Engine, ErrorKind, Key, Owned, ValueKind};
use madcawl_str::DomString;

use crate::convert::{Thrown, throw};
use crate::host::DomHost;

/// `v` as a value, or `Err` when it is the thrown sentinel.
pub(crate) fn got<E: Engine>(v: Owned<E::Value>) -> Result<Owned<E::Value>, Thrown> {
    if E::is_thrown(v.get()) {
        let _sentinel = v.into_raw();
        return Err(Thrown);
    }
    Ok(v)
}

pub(crate) fn is_object<E: Engine>(e: &E, v: E::Value) -> bool {
    matches!(e.kind(v), ValueKind::Object | ValueKind::Function)
}

/// Member `name` of `obj`; `None` when it is undefined.
pub(crate) fn member<E: Engine>(e: &mut E, h: &mut DomHost<E>, obj: E::Value, name: &str) -> Result<Option<Owned<E::Value>>, Thrown> {
    let v = got::<E>(e.get(h, obj, Key::Str(&DomString::from_wtf8(name.as_bytes()))))?;
    if e.kind(v.get()) == ValueKind::Undefined {
        e.release(v);
        return Ok(None);
    }
    Ok(Some(v))
}

/// Member `name` of the dictionary `dict` converted by `f`; `None` when
/// it is undefined.
pub(crate) fn convert<E: Engine, T>(
    e: &mut E,
    h: &mut DomHost<E>,
    dict: E::Value,
    name: &str,
    f: impl FnOnce(&mut E, &mut DomHost<E>, E::Value) -> Result<T, Thrown>,
) -> Result<Option<T>, Thrown> {
    let Some(v) = member(e, h, dict, name)? else { return Ok(None) };
    let r = f(e, h, v.get());
    e.release(v);
    r.map(Some)
}

/// A dictionary argument: `None` for undefined or null (every member
/// left out), or a TypeError for what is not an object.
pub(crate) fn dictionary<E: Engine>(e: &mut E, v: E::Value, what: &str) -> Result<Option<E::Value>, Thrown> {
    match e.kind(v) {
        ValueKind::Undefined | ValueKind::Null => Ok(None),
        ValueKind::Object | ValueKind::Function => Ok(Some(v)),
        _ => Err(throw(e, ErrorKind::Type, &format!("{what} is not a dictionary"))),
    }
}

/// GetMethod(`v`, @@iterator) of an object `v`: `None` when undefined
/// or null, a TypeError when not callable.
pub(crate) fn iterator_method<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<Option<Owned<E::Value>>, Thrown> {
    let realm = h.realm;
    let method = got::<E>(e.iterator_method(h, realm, v))?;
    match e.kind(method.get()) {
        ValueKind::Undefined | ValueKind::Null => {
            e.release(method);
            Ok(None)
        }
        _ if e.is_callable(method.get()) => Ok(Some(method)),
        _ => {
            e.release(method);
            Err(throw(e, ErrorKind::Type, "@@iterator is not a function"))
        }
    }
}

/// Whether to read the next item of a sequence.
pub(crate) enum Flow {
    Next,
    /// Stop here and close the iterator.
    Stop,
}

/// Read the iterable `v` item by item through `method`, its @@iterator,
/// as Web IDL makes a sequence, passing each item to `f` (borrowed for
/// the call).
pub(crate) fn for_each_item<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    v: E::Value,
    method: E::Value,
    mut f: impl FnMut(&mut E, &mut DomHost<E>, E::Value) -> Result<Flow, Thrown>,
) -> Result<(), Thrown> {
    let it = got::<E>(e.call(h, method, v, &[]))?;
    if !is_object(e, it.get()) {
        e.release(it);
        return Err(throw(e, ErrorKind::Type, "the iterator is not an object"));
    }
    let next = match got::<E>(e.get(h, it.get(), Key::Str(&DomString::from_wtf8(b"next")))) {
        Ok(n) => n,
        Err(t) => {
            e.release(it);
            return Err(t);
        }
    };
    let r = loop {
        let Some(item) = (match step(e, h, it.get(), next.get()) {
            Ok(item) => item,
            Err(t) => break Err(t),
        }) else {
            break Ok(());
        };
        let flow = f(e, h, item.get());
        e.release(item);
        match flow {
            Ok(Flow::Next) => {}
            Ok(Flow::Stop) => break close(e, h, it.get()),
            Err(t) => break Err(t),
        }
    };
    e.release(next);
    e.release(it);
    r
}

/// IteratorStep and IteratorValue: the next item, or `None` when done.
fn step<E: Engine>(e: &mut E, h: &mut DomHost<E>, it: E::Value, next: E::Value) -> Result<Option<Owned<E::Value>>, Thrown> {
    let s = got::<E>(e.call(h, next, it, &[]))?;
    if !is_object(e, s.get()) {
        e.release(s);
        return Err(throw(e, ErrorKind::Type, "the iterator result is not an object"));
    }
    let r = match convert(e, h, s.get(), "done", |e, _, d| Ok(e.to_boolean(d))) {
        Ok(Some(true)) => Ok(None),
        Ok(_) => got::<E>(e.get(h, s.get(), Key::Str(&DomString::from_wtf8(b"value")))).map(Some),
        Err(t) => Err(t),
    };
    e.release(s);
    r
}

/// IteratorClose of `it` after a normal completion.
fn close<E: Engine>(e: &mut E, h: &mut DomHost<E>, it: E::Value) -> Result<(), Thrown> {
    let Some(ret) = member(e, h, it, "return")? else { return Ok(()) };
    if e.kind(ret.get()) == ValueKind::Null {
        e.release(ret);
        return Ok(());
    }
    let r = got::<E>(e.call(h, ret.get(), it, &[]));
    e.release(ret);
    let r = r?;
    let object = is_object(e, r.get());
    e.release(r);
    if object { Ok(()) } else { Err(throw(e, ErrorKind::Type, "the iterator's return gave no object")) }
}
