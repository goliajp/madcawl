use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_js::Engine;
use madcawl_str::DomString;

use crate::convert::{Thrown, throw_dom};
use crate::host::DomHost;

pub(crate) use super::collections::child_nodes;

pub(crate) fn node_type<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<u16, Thrown> {
    Ok(match h.dom.kind(this) {
        NodeKind::Element => 1,
        NodeKind::Text => 3,
        NodeKind::ProcessingInstruction => 7,
        NodeKind::Comment => 8,
        NodeKind::Document => 9,
        NodeKind::Doctype => 10,
        NodeKind::Fragment => 11,
    })
}

pub(crate) fn node_name<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    let dom = &h.dom;
    Ok(match dom.kind(this) {
        NodeKind::Element => super::element::html_upper(dom, this),
        NodeKind::Text => DomString::from_wtf8(b"#text"),
        NodeKind::Comment => DomString::from_wtf8(b"#comment"),
        NodeKind::Document => DomString::from_wtf8(b"#document"),
        NodeKind::Fragment => DomString::from_wtf8(b"#document-fragment"),
        NodeKind::Doctype => dom.doctype(this).expect("a doctype")[0].clone(),
        NodeKind::ProcessingInstruction => dom.pi_target(this).expect("a target").to_dom_string(),
    })
}

pub(crate) fn is_connected<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<bool, Thrown> {
    Ok(h.dom.is_connected(this))
}

pub(crate) fn owner_document<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok((h.dom.kind(this) != NodeKind::Document).then(|| h.dom.owner_document(this)))
}

pub(crate) fn parent_node<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok(h.dom.parent(this))
}

pub(crate) fn parent_element<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok(h.dom.parent(this).filter(|&p| h.dom.kind(p) == NodeKind::Element))
}

pub(crate) fn has_child_nodes<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<bool, Thrown> {
    Ok(h.dom.first_child(this).is_some())
}

pub(crate) fn first_child<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok(h.dom.first_child(this))
}

pub(crate) fn last_child<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok(h.dom.last_child(this))
}

pub(crate) fn previous_sibling<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok(h.dom.previous_sibling(this))
}

pub(crate) fn next_sibling<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
    Ok(h.dom.next_sibling(this))
}

pub(crate) fn text_content<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<DomString>, Thrown> {
    let dom = &h.dom;
    Ok(match dom.kind(this) {
        NodeKind::Document | NodeKind::Doctype => None,
        k if k.is_char_data() => dom.text(this).cloned(),
        _ => {
            let mut b = Vec::new();
            for n in dom.descendants(this) {
                if dom.kind(n) == NodeKind::Text {
                    dom.text(n).expect("a text node").write_wtf8(&mut b);
                }
            }
            Some(DomString::from_wtf8(&b))
        }
    })
}

pub(crate) fn set_text_content<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    this: NodeId,
    value: Option<DomString>,
) -> Result<(), Thrown> {
    let value = value.unwrap_or_else(DomString::empty);
    let dom = &mut h.dom;
    match dom.kind(this) {
        NodeKind::Document | NodeKind::Doctype => Ok(()),
        k if k.is_char_data() => {
            if dom.set_text(this, value) { Ok(()) } else { Err(full(e, h)) }
        }
        _ => {
            while let Some(c) = dom.first_child(this) {
                dom.remove_node(c);
            }
            if value.is_empty() {
                return Ok(());
            }
            let Some(t) = dom.new_text(value) else { return Err(full(e, h)) };
            dom.insert_node(this, t, None);
            Ok(())
        }
    }
}

pub(crate) fn contains<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId, other: Option<NodeId>) -> Result<bool, Thrown> {
    Ok(other.is_some_and(|o| h.dom.is_inclusive_ancestor_of(this, o)))
}

pub(crate) fn insert_before<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    this: NodeId,
    node: NodeId,
    child: Option<NodeId>,
) -> Result<NodeId, Thrown> {
    if let Err((name, why)) = pre_insert_check(&h.dom, this, node, child) {
        return Err(throw_dom(e, h, name, why));
    }
    let dom = &mut h.dom;
    let before = if child == Some(node) { dom.next_sibling(node) } else { child };
    if dom.kind(node) == NodeKind::Fragment {
        while let Some(c) = dom.first_child(node) {
            dom.insert_node(this, c, before);
        }
    } else {
        dom.insert_node(this, node, before);
    }
    Ok(node)
}

pub(crate) fn append_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, node: NodeId) -> Result<NodeId, Thrown> {
    insert_before(e, h, this, node, None)
}

pub(crate) fn remove_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, child: NodeId) -> Result<NodeId, Thrown> {
    if h.dom.parent(child) != Some(this) {
        return Err(throw_dom(e, h, "NotFoundError", "the node is not a child of this node"));
    }
    h.dom.remove_node(child);
    Ok(child)
}

fn full<E: Engine>(e: &mut E, h: &mut DomHost<E>) -> Thrown {
    throw_dom(e, h, "QuotaExceededError", "the document holds all the nodes it may")
}

/// The DOM Standard's pre-insertion validity, for the node kinds there
/// are so far: the exception's name and message when it fails.
fn pre_insert_check(dom: &Dom, parent: NodeId, node: NodeId, child: Option<NodeId>) -> Result<(), (&'static str, &'static str)> {
    let hierarchy = |why| Err(("HierarchyRequestError", why));
    let pk = dom.kind(parent);
    if !pk.is_container() {
        return hierarchy("the parent cannot have children");
    }
    if dom.is_inclusive_ancestor_of(node, parent) {
        return hierarchy("the node is an ancestor of the parent");
    }
    if child.is_some_and(|c| dom.parent(c) != Some(parent)) {
        return Err(("NotFoundError", "the child is not a child of the parent"));
    }
    let nk = dom.kind(node);
    if nk == NodeKind::Document
        || (nk == NodeKind::Text && pk == NodeKind::Document)
        || (nk == NodeKind::Doctype && pk != NodeKind::Document)
    {
        return hierarchy("the node cannot go there");
    }
    if pk == NodeKind::Document {
        let elements = |n: NodeId| {
            if dom.kind(n) == NodeKind::Fragment {
                dom.children(n).filter(|&c| dom.kind(c) == NodeKind::Element).count()
            } else {
                (dom.kind(n) == NodeKind::Element) as usize
            }
        };
        let has = dom.children(parent).any(|c| c != node && dom.kind(c) == NodeKind::Element);
        if elements(node) > 1 || (elements(node) == 1 && has) {
            return hierarchy("a document has one element child");
        }
    }
    Ok(())
}
