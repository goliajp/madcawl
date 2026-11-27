//! The Performance Timeline's PerformanceObserver: its callback is a
//! member of the observer (`details`), and `observe` checks its options
//! as the standard does, an observer keeping the kind its first call
//! gave it. No entry type is supported yet, so an observer is never
//! registered for any, its buffer stays empty and its callback is never
//! called.

use madcawl_js::{Engine, ErrorKind, Key, Owned, ValueKind};
use madcawl_str::DomString;

use crate::convert::{Any, Thrown, array_of, throw, throw_dom};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

/// What the first `observe` made an observer: of several entry types
/// (`entryTypes`) or of one (`type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Multiple,
    Single,
}

pub(crate) fn constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, callback: E::Value) -> Result<SideRef, Thrown> {
    if !e.is_callable(callback) {
        return Err(throw(e, ErrorKind::Type, "the PerformanceObserver callback is not a function"));
    }
    let r = h.sides.add(SideData::Observer(None));
    h.set_value(e, r, callback);
    Ok(r)
}

/// The most entry types `observe` reads of an `entryTypes` sequence; the
/// rest are left unread and the iterator closed. No entry type is
/// supported, so the ones left out would have been ignored anyway.
const MAX_TYPES: usize = 64;

/// `v` as a value, or `Err` when it is the thrown sentinel.
fn got<E: Engine>(v: Owned<E::Value>) -> Result<Owned<E::Value>, Thrown> {
    if E::is_thrown(v.get()) {
        let _sentinel = v.into_raw();
        return Err(Thrown);
    }
    Ok(v)
}

fn is_object<E: Engine>(e: &E, v: E::Value) -> bool {
    matches!(e.kind(v), ValueKind::Object | ValueKind::Function)
}

/// Member `name` of `obj`; `None` when it is undefined.
fn member<E: Engine>(e: &mut E, h: &mut DomHost<E>, obj: E::Value, name: &str) -> Result<Option<Owned<E::Value>>, Thrown> {
    let v = got::<E>(e.get(h, obj, Key::Str(&DomString::from_wtf8(name.as_bytes()))))?;
    if e.kind(v.get()) == ValueKind::Undefined {
        e.release(v);
        return Ok(None);
    }
    Ok(Some(v))
}

/// Member `name` of the dictionary `dict` converted by `f`; `None` when
/// it is undefined.
fn convert<E: Engine, T>(
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

/// A `sequence<DOMString>` from the iterable `v`, by Web IDL's iterator
/// protocol, read to `MAX_TYPES` items.
fn strings<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<Vec<DomString>, Thrown> {
    if !is_object(e, v) {
        return Err(throw(e, ErrorKind::Type, "entryTypes is not a sequence"));
    }
    let realm = h.realm;
    let method = got::<E>(e.iterator_method(h, realm, v))?;
    if !e.is_callable(method.get()) {
        e.release(method);
        return Err(throw(e, ErrorKind::Type, "entryTypes is not iterable"));
    }
    let it = got::<E>(e.call(h, method.get(), v, &[]));
    e.release(method);
    let it = it?;
    let r = if is_object(e, it.get()) { iterate(e, h, it.get()) } else { Err(throw(e, ErrorKind::Type, "the iterator is not an object")) };
    e.release(it);
    r
}

fn iterate<E: Engine>(e: &mut E, h: &mut DomHost<E>, it: E::Value) -> Result<Vec<DomString>, Thrown> {
    let next = got::<E>(e.get(h, it, Key::Str(&DomString::from_wtf8(b"next"))))?;
    let mut out = Vec::new();
    let r = loop {
        if out.len() == MAX_TYPES {
            break close(e, h, it);
        }
        let step = match got::<E>(e.call(h, next.get(), it, &[])) {
            Ok(s) if is_object(e, s.get()) => s,
            Ok(s) => {
                e.release(s);
                break Err(throw(e, ErrorKind::Type, "the iterator result is not an object"));
            }
            Err(t) => break Err(t),
        };
        let done = convert(e, h, step.get(), "done", |e, _, d| Ok(e.to_boolean(d)));
        let item = match done {
            Ok(Some(true)) => None,
            Ok(_) => Some(got::<E>(e.get(h, step.get(), Key::Str(&DomString::from_wtf8(b"value"))))),
            Err(t) => Some(Err(t)),
        };
        e.release(step);
        let Some(item) = item else { break Ok(()) };
        let s = item.and_then(|v| {
            let s = e.to_string(h, v.get());
            e.release(v);
            s.ok_or(Thrown)
        });
        match s {
            Ok(s) => out.push(s),
            Err(t) => break Err(t),
        }
    };
    e.release(next);
    r.map(|()| out)
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

/// The PerformanceObserverInit dictionary, its members converted in
/// lexicographic order: whether `buffered` or `durationThreshold` was
/// given, `entryTypes`, `type`.
struct Options {
    others: bool,
    types: Option<Vec<DomString>>,
    ty: Option<DomString>,
}

fn options<E: Engine>(e: &mut E, h: &mut DomHost<E>, init: E::Value) -> Result<Options, Thrown> {
    match e.kind(init) {
        ValueKind::Undefined | ValueKind::Null => return Ok(Options { others: false, types: None, ty: None }),
        ValueKind::Object | ValueKind::Function => {}
        _ => return Err(throw(e, ErrorKind::Type, "the observe options are not a dictionary")),
    }
    let buffered = convert(e, h, init, "buffered", |e, _, v| Ok(e.to_boolean(v)))?;
    let threshold = convert(e, h, init, "durationThreshold", |e, h, v| match e.to_number(h, v) {
        Some(n) if n.is_finite() => Ok(n),
        Some(_) => Err(throw(e, ErrorKind::Type, "durationThreshold is not a finite number")),
        None => Err(Thrown),
    })?;
    let types = convert(e, h, init, "entryTypes", strings)?;
    let ty = convert(e, h, init, "type", |e, h, v| e.to_string(h, v).ok_or(Thrown))?;
    Ok(Options { others: buffered.is_some() || threshold.is_some(), types, ty })
}

/// Checks the options and the observer's kind. No entry type is
/// supported, so what remains of the standard's steps registers nothing.
pub(crate) fn observe<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, init: E::Value) -> Result<(), Thrown> {
    let o = options(e, h, init)?;
    match (&o.types, &o.ty) {
        (None, None) => return Err(throw(e, ErrorKind::Type, "observe needs entryTypes or type")),
        (Some(_), Some(_)) => return Err(throw(e, ErrorKind::Type, "entryTypes cannot be given with type")),
        (Some(_), None) if o.others => return Err(throw(e, ErrorKind::Type, "entryTypes cannot be given with buffered or durationThreshold")),
        _ => {}
    }
    let asked = if o.types.is_some() { Kind::Multiple } else { Kind::Single };
    let SideData::Observer(kind) = &mut h.sides.get_mut(this).data else { unreachable!("an observer") };
    match *kind.get_or_insert(asked) {
        k if k == asked => Ok(()),
        Kind::Single => Err(throw_dom(e, h, "InvalidModificationError", "the observer was made to observe one type")),
        Kind::Multiple => Err(throw_dom(e, h, "InvalidModificationError", "the observer was made to observe entryTypes")),
    }
}

pub(crate) fn disconnect<E: Engine>(_: &mut E, _: &mut DomHost<E>, _: SideRef) -> Result<(), Thrown> {
    Ok(())
}

pub(crate) fn take_records<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<Any<E>, Thrown> {
    Ok(Any(array_of(e, h, Vec::new())))
}
