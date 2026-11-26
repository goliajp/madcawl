//! The Attr interface's members (`attr.rs` has the object), and the
//! members of Node it answers itself: as an attribute, it has no parent,
//! no children and no siblings, and its text is its value.

use madcawl_dom::NodeId;
use madcawl_html::AttrNamespace;
use madcawl_js::Engine;
use madcawl_str::DomString;

use crate::convert::Thrown;
use crate::host::DomHost;
use crate::side::SideRef;

/// The namespaces an attribute of this DOM can be in, by URI.
pub(crate) const NAMESPACES: [(AttrNamespace, &str); 3] = [
    (AttrNamespace::XLink, "http://www.w3.org/1999/xlink"),
    (AttrNamespace::Xml, "http://www.w3.org/XML/1998/namespace"),
    (AttrNamespace::Xmlns, "http://www.w3.org/2000/xmlns/"),
];

pub(crate) fn namespace_uri<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Option<DomString>, Thrown> {
    let ns = h.attr(this).ns;
    Ok(NAMESPACES.iter().find(|(n, _)| *n == ns).map(|(_, uri)| DomString::from_wtf8(uri.as_bytes())))
}

pub(crate) fn prefix<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Option<DomString>, Thrown> {
    Ok(h.attr(this).prefix.map(|p| p.to_dom_string()))
}

pub(crate) fn local_name<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    Ok(h.attr(this).local.to_dom_string())
}

/// The qualified name.
pub(crate) fn name<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    let a = h.attr(this);
    let mut b = Vec::new();
    if let Some(p) = a.prefix {
        p.write_wtf8(&mut b);
        b.push(b':');
    }
    a.local.write_wtf8(&mut b);
    Ok(DomString::from_wtf8(&b))
}

pub(crate) fn value<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    let Some(el) = h.attr(this).element() else { return Ok(h.attr(this).value.clone()) };
    let k = h.place_of(el, this).expect("an attribute on its element");
    Ok(h.dom.attributes(el).nth(k).expect("the attribute").value.clone())
}

/// The value, on the element when the attribute is on one.
pub(crate) fn set_value<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, value: DomString) -> Result<(), Thrown> {
    let Some(el) = h.attr(this).element() else {
        h.attr_mut(this).value = value;
        return Ok(());
    };
    let k = h.place_of(el, this).expect("an attribute on its element");
    let mut b = Vec::new();
    value.write_wtf8(&mut b);
    let prefix = h.attr(this).prefix;
    if h.dom.set_attribute_at(el, k, prefix, &b) { Ok(()) } else { Err(super::attributes::full(e, h)) }
}

pub(crate) fn owner_element<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Option<NodeId>, Thrown> {
    Ok(h.attr(this).element())
}

pub(crate) fn specified<E: Engine>(_: &mut E, _: &mut DomHost<E>, _: SideRef) -> Result<bool, Thrown> {
    Ok(true)
}

pub(crate) fn node_type<E: Engine>(_: &mut E, _: &mut DomHost<E>, _: SideRef) -> Result<u16, Thrown> {
    Ok(2)
}

pub(crate) fn node_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    name(e, h, this)
}

pub(crate) fn is_connected<E: Engine>(_: &mut E, _: &mut DomHost<E>, _: SideRef) -> Result<bool, Thrown> {
    Ok(false)
}

/// The element's node document, or the document for one on no element.
pub(crate) fn owner_document<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Option<NodeId>, Thrown> {
    match h.attr(this).element() {
        Some(el) => super::node::owner_document(e, h, el),
        None => Ok(Some(h.dom.document())),
    }
}

pub(crate) fn none<E: Engine>(_: &mut E, _: &mut DomHost<E>, _: SideRef) -> Result<Option<NodeId>, Thrown> {
    Ok(None)
}

pub(crate) use {
    none as parent_node, none as parent_element, none as first_child, none as last_child, none as previous_sibling,
    none as next_sibling,
};

pub(crate) fn has_child_nodes<E: Engine>(_: &mut E, _: &mut DomHost<E>, _: SideRef) -> Result<bool, Thrown> {
    Ok(false)
}

pub(crate) fn text_content<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Option<DomString>, Thrown> {
    value(e, h, this).map(Some)
}

/// Null is the empty string.
pub(crate) fn set_text_content<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, v: Option<DomString>) -> Result<(), Thrown> {
    set_value(e, h, this, v.unwrap_or_else(DomString::empty))
}
