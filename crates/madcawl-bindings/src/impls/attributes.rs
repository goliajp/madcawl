//! Element's attribute members, and what `NamedNodeMap` shares with
//! them. Attributes are found by qualified name, lowercased first on an
//! HTML element: setting one changes the first attribute of that name,
//! in a namespace or not, and adds one in no namespace, the whole name
//! its local name, when there is none. Taking an attribute off goes through
//! [`take_off`], so that its `Attr` keeps its value.

use madcawl_dom::NodeId;
use madcawl_html::AttrNamespace;
use madcawl_js::{Engine, Owned, ValueKind};
use madcawl_str::{Atom, DomString, atom};

use crate::convert::{IntoJs, Thrown, array_of, throw_dom};
use crate::host::DomHost;
use crate::side::SideRef;

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
pub(crate) fn find<E: Engine>(h: &DomHost<E>, el: NodeId, name: &DomString) -> Option<usize> {
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
pub(crate) fn bad_name(b: &[u8]) -> bool {
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

pub(crate) fn set<E: Engine>(e: &mut E, h: &mut DomHost<E>, el: NodeId, local: Atom, value: &DomString) -> Result<(), Thrown> {
    let mut v = Vec::new();
    value.write_wtf8(&mut v);
    let new_src = local == atom!("src") && h.dom.attribute(el, local).is_none();
    if !h.dom.set_attribute(el, local, &v) {
        return Err(full(e, h));
    }
    if new_src {
        h.script_src_added(e, el);
    }
    Ok(())
}

pub(crate) fn set_attribute<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, name: DomString, value: DomString) -> Result<(), Thrown> {
    let n = checked_name(e, h, this, &name)?;
    let Some(k) = find(h, this, &name) else {
        let local = h.names.hold(&n);
        return set(e, h, this, local, &value);
    };
    let prefix = h.dom.attributes(this).nth(k).expect("the attribute found").prefix;
    let mut v = Vec::new();
    value.write_wtf8(&mut v);
    if h.dom.set_attribute_at(this, k, prefix, &v) { Ok(()) } else { Err(full(e, h)) }
}

pub(crate) fn remove_attribute<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, name: DomString) -> Result<(), Thrown> {
    if let Some(k) = find(h, this, &name) {
        take_off(e, h, this, k);
    }
    Ok(())
}

/// Take attribute `k` off `el`; its `Attr`, if it has one, keeps the
/// value it had.
pub(crate) fn take_off<E: Engine>(e: &mut E, h: &mut DomHost<E>, el: NodeId, k: usize) -> Option<SideRef> {
    let r = h.detach_attr(e, el, k);
    h.dom.remove_attribute(el, k);
    r
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
            take_off(e, h, this, k);
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

/// The namespace a namespace argument names; `None` for one no
/// attribute in this DOM can be in.
fn namespace(ns: Option<DomString>) -> Option<AttrNamespace> {
    let Some(ns) = ns.filter(|s| !s.is_empty()) else { return Some(AttrNamespace::None) };
    let mut b = Vec::new();
    ns.write_wtf8(&mut b);
    super::attr::NAMESPACES.iter().find(|(_, uri)| uri.as_bytes() == b.as_slice()).map(|&(n, _)| n)
}

/// The place of `el`'s attribute in namespace `ns` named `local`.
pub(crate) fn find_ns<E: Engine>(h: &DomHost<E>, el: NodeId, ns: Option<DomString>, local: &DomString) -> Option<usize> {
    let ns = namespace(ns)?;
    let mut b = Vec::new();
    local.write_wtf8(&mut b);
    let local = Atom::lookup(&b)?;
    h.dom.attributes(el).position(|a| a.ns == ns && a.local == local)
}

/// The standard's "set an attribute": `attr` goes on `el` in the place
/// of the one of its namespace and local name, which is returned, or
/// after the others. Past the byte limit the attribute is left off when
/// it found no slot, or put on with its value cut short, and `attr` is
/// on the element exactly when the attribute is.
pub(crate) fn set_node<E: Engine>(e: &mut E, h: &mut DomHost<E>, el: NodeId, attr: SideRef) -> Result<Option<SideRef>, Thrown> {
    let a = h.attr(attr);
    match a.element() {
        Some(x) if x == el => return Ok(Some(attr)),
        Some(_) => return Err(throw_dom(e, h, "InUseAttributeError", "the attribute is on another element")),
        None => {}
    }
    let (ns, prefix, local) = (a.ns, a.prefix, a.local);
    let mut v = Vec::new();
    a.value.write_wtf8(&mut v);
    let at = h.dom.attributes(el).position(|x| x.ns == ns && x.local == local);
    let (old, done) = match at {
        Some(k) => {
            let old = h.attr_of(el, k);
            h.detach_attr(e, el, k);
            (Some(old), h.dom.set_attribute_at(el, k, prefix, &v))
        }
        None => (None, h.dom.add_attribute(el, ns, prefix, local, &v)),
    };
    if h.place_of(el, attr).is_some() {
        h.attach_attr(e, attr, el);
    }
    if done { Ok(old) } else { Err(full(e, h)) }
}

pub(crate) fn attributes<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<SideRef, Thrown> {
    Ok(h.attr_map(this))
}

pub(crate) fn get_attribute_node<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId, name: DomString) -> Result<Option<SideRef>, Thrown> {
    Ok(find(h, this, &name).map(|k| h.attr_of(this, k)))
}

pub(crate) fn get_attribute_node_ns<E: Engine>(
    _: &mut E,
    h: &mut DomHost<E>,
    this: NodeId,
    ns: Option<DomString>,
    local: DomString,
) -> Result<Option<SideRef>, Thrown> {
    Ok(find_ns(h, this, ns, &local).map(|k| h.attr_of(this, k)))
}

pub(crate) fn set_attribute_node<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, attr: SideRef) -> Result<Option<SideRef>, Thrown> {
    set_node(e, h, this, attr)
}

pub(crate) fn set_attribute_node_ns<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, attr: SideRef) -> Result<Option<SideRef>, Thrown> {
    set_node(e, h, this, attr)
}

pub(crate) fn remove_attribute_node<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, attr: SideRef) -> Result<SideRef, Thrown> {
    match h.place_of(this, attr).filter(|_| h.attr(attr).element() == Some(this)) {
        Some(k) => Ok(take_off(e, h, this, k).expect("the attribute's own Attr")),
        None => Err(throw_dom(e, h, "NotFoundError", "the attribute is not on this element")),
    }
}
