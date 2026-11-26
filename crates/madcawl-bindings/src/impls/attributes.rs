//! Element's attribute members. Attributes are found by qualified name,
//! lowercased first on an HTML element; setting one with a prefix is not
//! told apart yet: the name is the local name.

use madcawl_dom::NodeId;
use madcawl_js::{Engine, Owned, ValueKind};
use madcawl_str::{Atom, DomString, atom};

use crate::convert::{IntoJs, Thrown, array_of, throw_dom};
use crate::host::DomHost;

/// Strings, as an array.
pub(crate) struct Names(Vec<DomString>);

impl<E: Engine> IntoJs<E> for Names {
    fn into_js(self, e: &mut E, h: &mut DomHost<E>) -> Owned<E::Value> {
        let items = self.0.into_iter().map(|s| s.into_js(e, h)).collect();
        array_of(e, h, items)
    }
}

/// An attribute's qualified name.
fn qualified(prefix: Option<Atom>, local: Atom, out: &mut Vec<u8>) {
    out.clear();
    if let Some(p) = prefix {
        p.write_wtf8(out);
        out.push(b':');
    }
    local.write_wtf8(out);
}

/// The place of the first attribute of `el` named `name`, in the order
/// the DOM keeps them.
fn find<E: Engine>(h: &DomHost<E>, el: NodeId, name: &DomString) -> Option<usize> {
    let mut want = Vec::new();
    name.write_wtf8(&mut want);
    if h.dom.in_html_namespace(el) {
        want.make_ascii_lowercase();
    }
    let mut q = Vec::new();
    h.dom.attributes(el).position(|a| {
        qualified(a.prefix, a.local, &mut q);
        q == want
    })
}

pub(crate) fn get_attribute<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId, name: DomString) -> Result<Option<DomString>, Thrown> {
    Ok(find(h, this, &name).and_then(|k| h.dom.attributes(this).nth(k)).map(|a| a.value.clone()))
}

pub(crate) fn has_attribute<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId, name: DomString) -> Result<bool, Thrown> {
    Ok(find(h, this, &name).is_some())
}

pub(crate) fn has_attributes<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<bool, Thrown> {
    Ok(h.dom.attributes(this).next().is_some())
}

pub(crate) fn get_attribute_names<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Names, Thrown> {
    let mut q = Vec::new();
    Ok(Names(
        h.dom
            .attributes(this)
            .map(|a| {
                qualified(a.prefix, a.local, &mut q);
                DomString::from_wtf8(&q)
            })
            .collect(),
    ))
}

/// The names `setAttribute` refuses: empty, or holding a character no
/// attribute name can.
fn bad_name(b: &[u8]) -> bool {
    b.is_empty() || b.iter().any(|&c| matches!(c, b'\t' | b'\n' | 0x0C | b'\r' | b' ' | b'/' | b'>' | b'=' | 0))
}

/// `name` as an attribute's local name, lowercased on an HTML element;
/// an InvalidCharacterError for a name no attribute can have.
fn checked_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, el: NodeId, name: &DomString) -> Result<Vec<u8>, Thrown> {
    let mut n = Vec::new();
    name.write_wtf8(&mut n);
    if bad_name(&n) {
        return Err(throw_dom(e, h, "InvalidCharacterError", "not a valid attribute name"));
    }
    if h.dom.in_html_namespace(el) {
        n.make_ascii_lowercase();
    }
    Ok(n)
}

fn set<E: Engine>(e: &mut E, h: &mut DomHost<E>, el: NodeId, local: Atom, value: &DomString) -> Result<(), Thrown> {
    let mut v = Vec::new();
    value.write_wtf8(&mut v);
    if h.dom.set_attribute(el, local, &v) { Ok(()) } else { Err(full(e, h)) }
}

pub(crate) fn set_attribute<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, name: DomString, value: DomString) -> Result<(), Thrown> {
    let n = checked_name(e, h, this, &name)?;
    let local = h.names.hold(&n);
    set(e, h, this, local, &value)
}

pub(crate) fn remove_attribute<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId, name: DomString) -> Result<(), Thrown> {
    if let Some(k) = find(h, this, &name) {
        h.dom.remove_attribute(this, k);
    }
    Ok(())
}

/// `toggleAttribute`: `force` is the optional boolean, read as any so
/// that a missing one is told from false.
pub(crate) fn toggle_attribute<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, name: DomString, force: E::Value) -> Result<bool, Thrown> {
    let n = checked_name(e, h, this, &name)?;
    let force = (e.kind(force) != ValueKind::Undefined).then(|| e.to_boolean(force));
    match find(h, this, &name) {
        None if force == Some(false) => Ok(false),
        None => {
            let local = h.names.hold(&n);
            set(e, h, this, local, &DomString::empty()).map(|()| true)
        }
        Some(_) if force == Some(true) => Ok(true),
        Some(k) => {
            h.dom.remove_attribute(this, k);
            Ok(false)
        }
    }
}

pub(crate) fn set_id<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, value: DomString) -> Result<(), Thrown> {
    set(e, h, this, atom!("id"), &value)
}

pub(crate) fn class_name<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(h.dom.attribute(this, atom!("class")).cloned().unwrap_or_else(DomString::empty))
}

pub(crate) fn set_class_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, value: DomString) -> Result<(), Thrown> {
    set(e, h, this, atom!("class"), &value)
}

pub(crate) fn full<E: Engine>(e: &mut E, h: &mut DomHost<E>) -> Thrown {
    throw_dom(e, h, "QuotaExceededError", "the document holds all the bytes it may")
}
