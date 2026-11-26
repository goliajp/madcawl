//! DocumentFragment: a parent of nodes in no tree, whose children move
//! when it is inserted.

use madcawl_dom::NodeId;
use madcawl_js::Engine;
use madcawl_str::DomString;

use crate::convert::{Thrown, throw_dom};
use crate::host::DomHost;

pub(crate) use super::collections::{child_element_count, children, first_element_child, last_element_child};
pub(crate) use super::parent::{query_selector, query_selector_all};

pub(crate) fn constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>) -> Result<NodeId, Thrown> {
    match h.dom.new_fragment() {
        Some(n) => Ok(n),
        None => Err(throw_dom(e, h, "QuotaExceededError", "the document holds all the nodes it may")),
    }
}

pub(crate) fn get_element_by_id<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, id: DomString) -> Result<Option<NodeId>, Thrown> {
    super::document::get_element_by_id(e, h, this, id)
}
