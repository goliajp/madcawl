use madcawl_dom::{NodeId, NodeKind};
use madcawl_js::Engine;
use madcawl_str::{Atom, DomString};

use crate::convert::{Thrown, throw_dom};
use crate::host::DomHost;

pub(crate) use super::parent::{get_elements_by_tag_name, query_selector, query_selector_all};

pub(crate) fn document_element<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok(h.dom.children(this).find(|&c| h.dom.kind(c) == NodeKind::Element))
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
