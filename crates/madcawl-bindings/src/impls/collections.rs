//! NodeList and HTMLCollection, and the members that give them or walk
//! the element children and siblings.

use madcawl_dom::{NodeId, NodeKind};
use madcawl_js::Engine;
use madcawl_str::{Atom, DomString, atom};

use crate::convert::Thrown;
use crate::host::DomHost;
use crate::lists::Filter;
use crate::side::SideRef;

pub(crate) fn item<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, index: u32) -> Result<Option<NodeId>, Thrown> {
    Ok(h.list_item(this, index))
}

pub(crate) fn length<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<u32, Thrown> {
    Ok(h.list_len(this))
}

/// The first element whose id is `name`, or that is in the HTML
/// namespace and has a `name` attribute of that value.
pub(crate) fn named_item<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, name: DomString) -> Result<Option<NodeId>, Thrown> {
    if name.is_empty() {
        return Ok(None);
    }
    let mut b = Vec::new();
    name.write_wtf8(&mut b);
    let id = Atom::lookup(&b);
    let nodes = h.list_nodes(this);
    let dom = &h.dom;
    Ok(nodes.into_iter().find(|&n| {
        (id.is_some() && dom.element_id(n) == id)
            || (dom.in_html_namespace(n) && dom.attribute(n, atom!("name")).is_some_and(|v| *v == name))
    }))
}

pub(crate) fn child_nodes<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<SideRef, Thrown> {
    Ok(h.child_list(this, false))
}

pub(crate) fn children<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<SideRef, Thrown> {
    Ok(h.child_list(this, true))
}

/// The descendant elements whose qualified name is `name` (`*` for all),
/// compared in ASCII lower case for HTML elements.
pub(crate) fn get_elements_by_tag_name<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId, name: DomString) -> Result<SideRef, Thrown> {
    let mut want = Vec::new();
    name.write_wtf8(&mut want);
    let lower = want.to_ascii_lowercase().into_boxed_slice();
    Ok(h.live_list(this, Filter::Tag { name: want.into_boxed_slice(), lower }))
}

/// The descendant elements that have every class `names` lists.
pub(crate) fn get_elements_by_class_name<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId, names: DomString) -> Result<SideRef, Thrown> {
    let mut b = Vec::new();
    names.write_wtf8(&mut b);
    let list = b.split(|c| matches!(c, b'\t' | b'\n' | 0x0C | b'\r' | b' ')).filter(|t| !t.is_empty()).map(Box::from).collect();
    Ok(h.live_list(this, Filter::Classes(list)))
}

fn is_element<E: Engine>(h: &DomHost<E>, n: NodeId) -> bool {
    h.dom.kind(n) == NodeKind::Element
}

pub(crate) fn first_element_child<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok(h.dom.children(this).find(|&c| is_element(h, c)))
}

pub(crate) fn last_element_child<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok(std::iter::successors(h.dom.last_child(this), |&c| h.dom.previous_sibling(c)).find(|&c| is_element(h, c)))
}

pub(crate) fn child_element_count<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<u32, Thrown> {
    Ok(h.dom.children(this).filter(|&c| is_element(h, c)).count() as u32)
}

pub(crate) fn previous_element_sibling<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok(std::iter::successors(h.dom.previous_sibling(this), |&c| h.dom.previous_sibling(c)).find(|&c| is_element(h, c)))
}

pub(crate) fn next_element_sibling<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok(std::iter::successors(h.dom.next_sibling(this), |&c| h.dom.next_sibling(c)).find(|&c| is_element(h, c)))
}
