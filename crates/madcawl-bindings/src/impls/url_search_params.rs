//! The URL Standard's URLSearchParams: a list of name-value pairs, and
//! the URL whose query it is when it is a URL's `searchParams`, held by
//! a count both ways (a cycle the collector frees). The iterators are
//! ones over arrays of the pairs as they were when made.

use alt_url::form;
use madcawl_js::{Engine, ErrorKind, Key};

use crate::convert::{Any, Thrown, array_in, throw, usv, usv_string};
use crate::dict::{Flow, for_each_item, got, is_object, iterator_method};
use crate::event::{Held, Target};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

pub(crate) struct Params {
    pub list: Vec<(String, String)>,
    pub url: Option<Held>,
}

fn params<E: Engine>(h: &mut DomHost<E>, r: SideRef) -> &mut Params {
    match &mut h.sides.get_mut(r).data {
        SideData::SearchParams(p) => p,
        _ => unreachable!("a URLSearchParams's side object"),
    }
}

pub(crate) fn new_params<E: Engine>(h: &mut DomHost<E>, list: Vec<(String, String)>, url: Option<SideRef>) -> SideRef {
    let url = url.map(|u| h.hold_target(Target::Side(u)));
    h.sides.add(SideData::SearchParams(Box::new(Params { list, url })))
}

/// The list of `r`, the URL's query having changed.
pub(crate) fn replace_list<E: Engine>(h: &mut DomHost<E>, r: SideRef, list: Vec<(String, String)>) {
    params(h, r).list = list;
}

pub(crate) fn serialize(list: &[(String, String)]) -> String {
    let mut out = String::new();
    for (i, (k, v)) in list.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        form::append_pair(&mut out, k, v);
    }
    out
}

/// The standard's update steps: the URL's query is the list serialized.
fn update<E: Engine>(h: &mut DomHost<E>, r: SideRef) {
    let p = params(h, r);
    let Some(Held::Side(u)) = p.url else { return };
    let q = serialize(&p.list);
    let SideData::Url(state) = &mut h.sides.get_mut(u).data else { unreachable!("a URL's side object") };
    if let Some(new) = crate::url_parts::with_query(&state.url, (!q.is_empty()).then_some(q.as_str())) {
        state.url = new;
    }
}

/// A sequence of USVString of exactly two items.
fn pair<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<(String, String), Thrown> {
    let method = if is_object(e, v) { iterator_method(e, h, v)? } else { None };
    let Some(method) = method else { return Err(throw(e, ErrorKind::Type, "a URLSearchParams pair is not a sequence")) };
    let mut items = Vec::new();
    let r = for_each_item(e, h, v, method.get(), |e, h, item| {
        items.push(usv_string(e, h, item)?);
        Ok(Flow::Next)
    });
    e.release(method);
    r?;
    match <[String; 2]>::try_from(items) {
        Ok([k, v]) => Ok((k, v)),
        Err(_) => Err(throw(e, ErrorKind::Type, "a URLSearchParams pair has not two items")),
    }
}

/// The record `v`'s own enumerable string-keyed properties.
fn record<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<Vec<(String, String)>, Thrown> {
    let realm = h.realm;
    let arr = got::<E>(e.own_entries(h, realm, v))?;
    let mut strings = Vec::new();
    let mut i = 0;
    loop {
        let item = e.get(h, arr.get(), Key::Index(i));
        let s = e.as_string(item.get());
        e.release(item);
        let Some(s) = s else { break };
        strings.push(usv(&s));
        i += 1;
    }
    e.release(arr);
    let mut it = strings.into_iter();
    let mut out = Vec::new();
    while let (Some(k), Some(v)) = (it.next(), it.next()) {
        // a later value for the same key replaces the earlier one
        match out.iter_mut().find(|(name, _): &&mut (String, String)| *name == k) {
            Some(slot) => slot.1 = v,
            None => out.push((k, v)),
        }
    }
    Ok(out)
}

pub(crate) fn constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, init: E::Value) -> Result<SideRef, Thrown> {
    let list = if init == E::undefined() {
        Vec::new()
    } else if is_object(e, init) {
        match iterator_method(e, h, init)? {
            Some(method) => {
                let mut list = Vec::new();
                let r = for_each_item(e, h, init, method.get(), |e, h, item| {
                    list.push(pair(e, h, item)?);
                    Ok(Flow::Next)
                });
                e.release(method);
                r?;
                list
            }
            None => record(e, h, init)?,
        }
    } else {
        let s = usv_string(e, h, init)?;
        form::parse(s.strip_prefix('?').unwrap_or(&s).as_bytes()).into_owned().collect()
    };
    Ok(new_params(h, list, None))
}

pub(crate) fn size<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<u32, Thrown> {
    Ok(params(h, this).list.len() as u32)
}

pub(crate) fn append<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, name: String, value: String) -> Result<(), Thrown> {
    params(h, this).list.push((name, value));
    update(h, this);
    Ok(())
}

/// The value argument of `delete` and `has`: `None` when left out.
fn value_arg<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<Option<String>, Thrown> {
    if v == E::undefined() { Ok(None) } else { usv_string(e, h, v).map(Some) }
}

pub(crate) fn delete<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, name: String, value: E::Value) -> Result<(), Thrown> {
    let value = value_arg(e, h, value)?;
    params(h, this).list.retain(|(k, v)| *k != name || value.as_ref().is_some_and(|x| x != v));
    update(h, this);
    Ok(())
}

pub(crate) fn get<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, name: String) -> Result<Option<String>, Thrown> {
    Ok(params(h, this).list.iter().find(|(k, _)| *k == name).map(|(_, v)| v.clone()))
}

pub(crate) fn get_all<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, name: String) -> Result<Any<E>, Thrown> {
    let realm = h.caller_realm(e)?;
    let values: Vec<String> = params(h, this).list.iter().filter(|(k, _)| *k == name).map(|(_, v)| v.clone()).collect();
    let items = values.into_iter().map(|v| crate::convert::IntoJs::into_js(v, e, h)).collect();
    Ok(Any(array_in(e, realm, items)))
}

pub(crate) fn has<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, name: String, value: E::Value) -> Result<bool, Thrown> {
    let value = value_arg(e, h, value)?;
    Ok(params(h, this).list.iter().any(|(k, v)| *k == name && value.as_ref().is_none_or(|x| x == v)))
}

pub(crate) fn set<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, name: String, value: String) -> Result<(), Thrown> {
    let list = &mut params(h, this).list;
    match list.iter().position(|(k, _)| *k == name) {
        Some(i) => {
            list[i].1 = value;
            let mut seen = 0;
            list.retain(|(k, _)| {
                seen += usize::from(*k == name);
                *k != name || seen == 1
            });
        }
        None => list.push((name, value)),
    }
    update(h, this);
    Ok(())
}

/// Stable, by the names' UTF-16 code units.
pub(crate) fn sort<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<(), Thrown> {
    params(h, this).list.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
    update(h, this);
    Ok(())
}

pub(crate) fn to_string<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    Ok(serialize(&params(h, this).list))
}

/// What an iterator gives for a pair, made in the caller's realm.
type Item<E> = fn(&mut E, &mut DomHost<E>, <E as Engine>::Realm, String, String) -> madcawl_js::Owned<<E as Engine>::Value>;

/// An Array Iterator over an array of what `item` makes of each pair,
/// from the caller's realm's own `Array.prototype.values`.
fn iterate<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, item: Item<E>) -> Result<Any<E>, Thrown> {
    let realm = h.caller_realm(e)?;
    let list = params(h, this).list.clone();
    let items = list.into_iter().map(|(k, v)| item(e, h, realm, k, v)).collect();
    let arr = array_in(e, realm, items);
    let it = e.array_values(h, realm, arr.get());
    e.release(arr);
    Ok(Any(got::<E>(it)?))
}

fn string<E: Engine>(e: &mut E, h: &mut DomHost<E>, s: String) -> madcawl_js::Owned<E::Value> {
    crate::convert::IntoJs::into_js(s, e, h)
}

pub(crate) fn entries<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
    iterate(e, h, this, |e, h, realm, k, v| {
        let pair = vec![string(e, h, k), string(e, h, v)];
        array_in(e, realm, pair)
    })
}

pub(crate) fn keys<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
    iterate(e, h, this, |e, h, _, k, _| string(e, h, k))
}

pub(crate) fn values<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
    iterate(e, h, this, |e, h, _, _, v| string(e, h, v))
}

/// Calls `callback(value, name, params)` for each pair in the list as
/// it is at that index, the list read again after each call.
pub(crate) fn for_each<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, callback: E::Value, this_arg: E::Value) -> Result<(), Thrown> {
    if !e.is_callable(callback) {
        return Err(throw(e, ErrorKind::Type, "forEach takes a function"));
    }
    let me = h.wrap_side(e, this);
    let mut i = 0;
    let r = loop {
        let Some((k, v)) = params(h, this).list.get(i).cloned() else { break Ok(()) };
        let (k, v) = (string(e, h, k), string(e, h, v));
        let out = e.call(h, callback, this_arg, &[v.get(), k.get(), me.get()]);
        e.release(k);
        e.release(v);
        if let Err(t) = got::<E>(out).map(|o| e.release(o)) {
            break Err(t);
        }
        i += 1;
    };
    e.release(me);
    r
}
