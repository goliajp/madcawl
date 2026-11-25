use madcawl_dom::{NodeId, NodeKind};
use madcawl_js::Engine;
use madcawl_str::{Atom, DomString};

use crate::convert::{Thrown, throw_dom};
use crate::host::DomHost;

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
