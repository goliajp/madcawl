use madcawl_dom::{Dom, NodeId};
use madcawl_js::Engine;
use madcawl_str::{Atom, DomString};

use crate::convert::Thrown;
use crate::host::DomHost;

pub(crate) use super::collections::{
    child_element_count, children, first_element_child, get_elements_by_class_name, get_elements_by_tag_name,
    last_element_child, next_element_sibling, previous_element_sibling,
};
pub(crate) use super::attributes::{
    attributes, class_name, get_attribute, get_attribute_names, get_attribute_node, get_attribute_node_ns, has_attribute,
    has_attributes, remove_attribute, remove_attribute_node, set_attribute, set_attribute_node, set_attribute_node_ns,
    set_class_name, set_id, toggle_attribute,
};
pub(crate) use super::parent::{query_selector, query_selector_all};

/// An HTML element's name in ASCII upper case; others' as they are.
pub(crate) fn html_upper(dom: &Dom, el: NodeId) -> DomString {
    let mut b = Vec::new();
    dom.local_name(el).expect("an element").write_wtf8(&mut b);
    if dom.in_html_namespace(el) {
        b.make_ascii_uppercase();
    }
    DomString::from_wtf8(&b)
}

fn atom_string(a: Option<Atom>) -> DomString {
    a.map_or_else(DomString::empty, Atom::to_dom_string)
}

pub(crate) fn local_name<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(atom_string(h.dom.local_name(this)))
}

pub(crate) fn tag_name<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(html_upper(&h.dom, this))
}

pub(crate) fn id<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(atom_string(h.dom.element_id(this)))
}

pub(crate) fn remove<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<(), Thrown> {
    if h.dom.parent(this).is_some() {
        h.dom.remove_node(this);
    }
    Ok(())
}
