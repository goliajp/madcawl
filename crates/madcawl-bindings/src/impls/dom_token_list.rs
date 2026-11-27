//! The DOMTokenList interface, as an element's `classList`: the ordered
//! set of the tokens of its `class` attribute, read from the attribute
//! on each call, and written back to it by the update steps. One object
//! per element while it lives (`attr.rs`).

use std::collections::HashSet;

use madcawl_dom::NodeId;
use madcawl_js::{Engine, ErrorKind, ValueKind};
use madcawl_str::{DomString, atom};

use crate::convert::{Thrown, throw, throw_dom};
use crate::host::DomHost;
use crate::side::SideRef;

fn element<E: Engine>(h: &DomHost<E>, this: SideRef) -> NodeId {
    h.owning_element(this).expect("a token list's element")
}

fn is_space(u: u16) -> bool {
    matches!(u, 0x09 | 0x0A | 0x0C | 0x0D | 0x20)
}

/// The ordered set parser over the `class` attribute's value.
fn tokens<E: Engine>(h: &DomHost<E>, el: NodeId) -> Vec<Vec<u16>> {
    let mut out: Vec<Vec<u16>> = Vec::new();
    let Some(v) = h.dom.attribute(el, atom!("class")) else { return out };
    let units: Vec<u16> = v.units().collect();
    let mut seen: HashSet<&[u16]> = HashSet::new();
    for t in units.split(|&u| is_space(u)).filter(|t| !t.is_empty()) {
        if seen.insert(t) {
            out.push(t.to_vec());
        }
    }
    out
}

fn units(s: &DomString) -> Vec<u16> {
    s.units().collect()
}

/// A SyntaxError for an empty token, an InvalidCharacterError for one
/// with ASCII whitespace.
fn check<E: Engine>(e: &mut E, h: &mut DomHost<E>, t: &[u16]) -> Result<(), Thrown> {
    if t.is_empty() {
        return Err(throw_dom(e, h, "SyntaxError", "a token is empty"));
    }
    if t.iter().any(|&u| is_space(u)) {
        return Err(throw_dom(e, h, "InvalidCharacterError", "a token holds whitespace"));
    }
    Ok(())
}

/// The update steps: the set, serialized, becomes the attribute's
/// value, unless there is no attribute and the set is empty.
fn update<E: Engine>(e: &mut E, h: &mut DomHost<E>, el: NodeId, set: &[Vec<u16>]) -> Result<(), Thrown> {
    if set.is_empty() && h.dom.attribute(el, atom!("class")).is_none() {
        return Ok(());
    }
    let joined = set.join(&0x20);
    super::attributes::set(e, h, el, atom!("class"), &DomString::from_utf16(&joined))
}

pub(crate) fn length<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<u32, Thrown> {
    Ok(tokens(h, element(h, this)).len() as u32)
}

pub(crate) fn item<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, i: u32) -> Result<Option<DomString>, Thrown> {
    let set = tokens(h, element(h, this));
    Ok(set.get(i as usize).map(|t| DomString::from_utf16(t)))
}

pub(crate) fn contains<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, token: DomString) -> Result<bool, Thrown> {
    let t = units(&token);
    Ok(tokens(h, element(h, this)).contains(&t))
}

pub(crate) fn add<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, list: Vec<DomString>) -> Result<(), Thrown> {
    let list: Vec<Vec<u16>> = list.iter().map(units).collect();
    for t in &list {
        check(e, h, t)?;
    }
    let el = element(h, this);
    let mut set = tokens(h, el);
    let mut seen: HashSet<Vec<u16>> = set.iter().cloned().collect();
    for t in list {
        if seen.insert(t.clone()) {
            set.push(t);
        }
    }
    update(e, h, el, &set)
}

pub(crate) fn remove<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, list: Vec<DomString>) -> Result<(), Thrown> {
    let list: Vec<Vec<u16>> = list.iter().map(units).collect();
    for t in &list {
        check(e, h, t)?;
    }
    let el = element(h, this);
    let mut set = tokens(h, el);
    let gone: HashSet<&[u16]> = list.iter().map(Vec::as_slice).collect();
    set.retain(|t| !gone.contains(t.as_slice()));
    update(e, h, el, &set)
}

pub(crate) fn toggle<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, token: DomString, force: E::Value) -> Result<bool, Thrown> {
    let force = (e.kind(force) != ValueKind::Undefined).then(|| e.to_boolean(force));
    let t = units(&token);
    check(e, h, &t)?;
    let el = element(h, this);
    let mut set = tokens(h, el);
    if let Some(i) = set.iter().position(|x| *x == t) {
        if force == Some(true) {
            return Ok(true);
        }
        set.remove(i);
        update(e, h, el, &set)?;
        return Ok(false);
    }
    if force == Some(false) {
        return Ok(false);
    }
    set.push(t);
    update(e, h, el, &set)?;
    Ok(true)
}

pub(crate) fn replace<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, token: DomString, new: DomString) -> Result<bool, Thrown> {
    let (t, n) = (units(&token), units(&new));
    if t.is_empty() || n.is_empty() {
        return Err(throw_dom(e, h, "SyntaxError", "a token is empty"));
    }
    check(e, h, &t)?;
    check(e, h, &n)?;
    let el = element(h, this);
    let mut set = tokens(h, el);
    let Some(i) = set.iter().position(|x| *x == t) else { return Ok(false) };
    if t != n {
        // the ordered set's replace: the first of the two takes the new
        // token's place, the other goes
        let first = set.iter().position(|x| *x == n).map_or(i, |j| i.min(j));
        set[first] = n.clone();
        let mut seen = false;
        set.retain(|x| *x != n || !std::mem::replace(&mut seen, true));
        set.retain(|x| *x != t);
    }
    update(e, h, el, &set)?;
    Ok(true)
}

/// `classList` defines no supported tokens.
pub(crate) fn supports<E: Engine>(e: &mut E, _: &mut DomHost<E>, _: SideRef, _token: DomString) -> Result<bool, Thrown> {
    Err(throw(e, ErrorKind::Type, "classList has no supported tokens"))
}

pub(crate) fn value<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    Ok(h.dom.attribute(element(h, this), atom!("class")).cloned().unwrap_or_else(DomString::empty))
}

pub(crate) fn set_value<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, v: DomString) -> Result<(), Thrown> {
    let el = element(h, this);
    super::attributes::set(e, h, el, atom!("class"), &v)
}

pub(crate) use value as to_string;
