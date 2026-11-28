//! The Performance Timeline's PerformanceObserver: its callback is a
//! member of the observer (`details`), and `observe` checks its options
//! as the standard does, an observer keeping the kind its first call
//! gave it. No entry type is supported yet, so an observer is never
//! registered for any, its buffer stays empty and its callback is never
//! called.

use madcawl_js::{Engine, ErrorKind, ValueKind};
use madcawl_str::DomString;

use crate::convert::{Any, Thrown, array_of, throw, throw_dom};
use crate::dict::{Flow, convert, for_each_item, is_object, iterator_method};
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

/// A `sequence<DOMString>` from the iterable `v`, by Web IDL's iterator
/// protocol, read to `MAX_TYPES` items.
fn strings<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<Vec<DomString>, Thrown> {
    if !is_object(e, v) {
        return Err(throw(e, ErrorKind::Type, "entryTypes is not a sequence"));
    }
    let Some(method) = iterator_method(e, h, v)? else { return Err(throw(e, ErrorKind::Type, "entryTypes is not iterable")) };
    let mut out = Vec::new();
    let r = for_each_item(e, h, v, method.get(), |e, h, item| {
        out.push(e.to_string(h, item).ok_or(Thrown)?);
        Ok(if out.len() == MAX_TYPES { Flow::Stop } else { Flow::Next })
    });
    e.release(method);
    r.map(|()| out)
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
