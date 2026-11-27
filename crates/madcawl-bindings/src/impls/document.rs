use madcawl_dom::{NodeId, NodeKind};
use madcawl_js::Engine;
use madcawl_str::{Atom, DomString};

use crate::convert::{Thrown, throw_dom};
use crate::host::DomHost;

pub(crate) use super::collections::{
    child_element_count, children, first_element_child, get_elements_by_class_name, get_elements_by_tag_name,
    last_element_child,
};
pub(crate) use super::parent::{query_selector, query_selector_all};

pub(crate) fn document_element<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok(h.dom.children(this).find(|&c| h.dom.kind(c) == NodeKind::Element))
}

pub(crate) fn current_script<E: Engine>(_: &mut E, h: &mut DomHost<E>, _this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok(h.current_script())
}

/// The window's Location, for the window's own document; null for any
/// other, which has no browsing context.
pub(crate) fn location<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<crate::side::SideRef>, Thrown> {
    Ok(h.location_object().filter(|_| this == h.dom.document()))
}

/// `document.location = url`, forwarded to the Location's `href`; a
/// TypeError for a document with no Location.
pub(crate) fn set_location<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, url: DomString) -> Result<(), Thrown> {
    match location(e, h, this)? {
        Some(r) => crate::impls::location::set_href(e, h, r, url),
        None => Err(crate::convert::throw(e, madcawl_js::ErrorKind::Type, "the document has no Location to set")),
    }
}

/// The names `createElement` refuses: empty, or holding a character
/// no element name can (whitespace, `/`, `>`, NUL).
fn bad_name(b: &[u8]) -> bool {
    b.is_empty() || b.iter().any(|&c| matches!(c, b'\t' | b'\n' | 0x0C | b'\r' | b' ' | b'/' | b'>' | 0))
}

pub(crate) fn create_element<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: NodeId, local: DomString) -> Result<NodeId, Thrown> {
    let mut b = Vec::new();
    local.write_wtf8(&mut b);
    if bad_name(&b) {
        return Err(throw_dom(e, h, "InvalidCharacterError", "not a valid element name"));
    }
    b.make_ascii_lowercase();
    match h.dom.new_element(&b) {
        Some(n) => Ok(n),
        None => Err(full(e, h)),
    }
}

pub(crate) fn create_text_node<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: NodeId, data: DomString) -> Result<NodeId, Thrown> {
    match h.dom.new_text(data) {
        Some(n) => Ok(n),
        None => Err(full(e, h)),
    }
}

pub(crate) fn create_document_fragment<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: NodeId) -> Result<NodeId, Thrown> {
    super::document_fragment::constructor(e, h)
}

pub(crate) fn create_comment<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: NodeId, data: DomString) -> Result<NodeId, Thrown> {
    match h.dom.new_comment(data) {
        Some(n) => Ok(n),
        None => Err(full(e, h)),
    }
}

/// `createAttribute`: an `Attr` on no element, its name in ASCII lower
/// case (the document is an HTML document), refused as `setAttribute`
/// refuses one.
pub(crate) fn create_attribute<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: NodeId, local: DomString) -> Result<crate::side::SideRef, Thrown> {
    let mut n = Vec::new();
    local.write_wtf8(&mut n);
    if super::attributes::bad_name(&n) {
        return Err(throw_dom(e, h, "InvalidCharacterError", "not a valid attribute name"));
    }
    n.make_ascii_lowercase();
    let local = h.names.hold(&n);
    Ok(h.loose_attr(local))
}

pub(crate) fn get_element_by_id<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId, id: DomString) -> Result<Option<NodeId>, Thrown> {
    let mut b = Vec::new();
    id.write_wtf8(&mut b);
    // no element has an id no atom spells
    let Some(id) = Atom::lookup(&b).filter(|_| !b.is_empty()) else { return Ok(None) };
    let dom = &h.dom;
    Ok(dom
        .descendants(this)
        .find(|&n| dom.kind(n) == NodeKind::Element && dom.element_id(n) == Some(id)))
}

fn full<E: Engine>(e: &mut E, h: &mut DomHost<E>) -> Thrown {
    throw_dom(e, h, "QuotaExceededError", "the document holds all the nodes it may")
}

/// `document.write`: into the parser at its insertion point while a
/// script the parser stopped for runs; ignored otherwise (opening a new
/// document is not done yet).
pub(crate) fn write<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: NodeId, text: DomString) -> Result<(), Thrown> {
    h.document_write(e, &text);
    Ok(())
}

/// An uninitialized event: `initEvent` must run before it is dispatched.
pub(crate) fn create_event<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: NodeId, interface: DomString) -> Result<crate::side::SideRef, Thrown> {
    let mut b = Vec::new();
    interface.write_wtf8(&mut b);
    b.make_ascii_lowercase();
    let custom = &b[..] == b"customevent";
    if !custom && !matches!(&b[..], b"event" | b"events" | b"htmlevents" | b"svgevents") {
        return Err(crate::convert::throw_dom(e, h, "NotSupportedError", "no event interface of that name"));
    }
    let mut ev = crate::event::EventState::new(h.names.hold(b""), h.now_ms());
    (ev.initialized, ev.custom) = (false, custom);
    Ok(h.sides.add(crate::side::SideData::Event(ev)))
}
