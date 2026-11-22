// CODEGEN: tools/gen-bindings.py from crates/madcawl-bindings/idl/dom.webidl
//! The class tables of the bound interfaces, and the glue between
//! script values and the implementations in `impls`.

use std::marker::PhantomData;

use madcawl_js::{Attribute, ClassDef, ClassId, Constant, Engine, Operation, Owned};

use crate::convert as cv;
use crate::gc::NodeOps;
use crate::host::DomHost;
use crate::impls;

pub struct EventTargetClass<E>(PhantomData<E>);

impl<E: Engine> EventTargetClass<E> {
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "EventTarget",
        id: ClassId(1),
        last: ClassId(4),
        parent: None,
        attributes: &[],
        operations: &[
            Operation { name: "addEventListener", length: 2, call: event_target_add_event_listener::<E> },
            Operation { name: "removeEventListener", length: 2, call: event_target_remove_event_listener::<E> },
        ],
        constants: &[],
        interceptors: None,
        ops: None,
    };
}

fn event_target_add_event_listener<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &EventTargetClass::<E>::DEF)?;
        cv::need(e, args, 2)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::callback_or_null(e, cv::arg::<E>(args, 1))?;
        let a2 = cv::boolean_or(e, cv::arg::<E>(args, 2), false);
        impls::event_target::add_event_listener(e, h, this, a0, a1, a2)
    })
}

fn event_target_remove_event_listener<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &EventTargetClass::<E>::DEF)?;
        cv::need(e, args, 2)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::callback_or_null(e, cv::arg::<E>(args, 1))?;
        let a2 = cv::boolean_or(e, cv::arg::<E>(args, 2), false);
        impls::event_target::remove_event_listener(e, h, this, a0, a1, a2)
    })
}

pub struct NodeClass<E>(PhantomData<E>);

impl<E: Engine> NodeClass<E> {
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "Node",
        id: ClassId(2),
        last: ClassId(4),
        parent: Some(&EventTargetClass::<E>::DEF),
        attributes: &[
            Attribute { name: "nodeType", get: node_get_node_type::<E>, set: None },
            Attribute { name: "nodeName", get: node_get_node_name::<E>, set: None },
            Attribute { name: "isConnected", get: node_get_is_connected::<E>, set: None },
            Attribute { name: "ownerDocument", get: node_get_owner_document::<E>, set: None },
            Attribute { name: "parentNode", get: node_get_parent_node::<E>, set: None },
            Attribute { name: "parentElement", get: node_get_parent_element::<E>, set: None },
            Attribute { name: "firstChild", get: node_get_first_child::<E>, set: None },
            Attribute { name: "lastChild", get: node_get_last_child::<E>, set: None },
            Attribute { name: "previousSibling", get: node_get_previous_sibling::<E>, set: None },
            Attribute { name: "nextSibling", get: node_get_next_sibling::<E>, set: None },
            Attribute { name: "textContent", get: node_get_text_content::<E>, set: Some(node_set_text_content::<E>) },
        ],
        operations: &[
            Operation { name: "hasChildNodes", length: 0, call: node_has_child_nodes::<E> },
            Operation { name: "contains", length: 1, call: node_contains::<E> },
            Operation { name: "insertBefore", length: 2, call: node_insert_before::<E> },
            Operation { name: "appendChild", length: 1, call: node_append_child::<E> },
            Operation { name: "removeChild", length: 1, call: node_remove_child::<E> },
        ],
        constants: &[
            Constant { name: "ELEMENT_NODE", value: 1.0 },
            Constant { name: "TEXT_NODE", value: 3.0 },
            Constant { name: "PROCESSING_INSTRUCTION_NODE", value: 7.0 },
            Constant { name: "COMMENT_NODE", value: 8.0 },
            Constant { name: "DOCUMENT_NODE", value: 9.0 },
            Constant { name: "DOCUMENT_TYPE_NODE", value: 10.0 },
            Constant { name: "DOCUMENT_FRAGMENT_NODE", value: 11.0 },
        ],
        interceptors: None,
        ops: Some(&NodeOps::<E>::OPS),
    };
}

fn node_get_node_type<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::node_type(e, h, this)
    })
}

fn node_get_node_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::node_name(e, h, this)
    })
}

fn node_get_is_connected<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::is_connected(e, h, this)
    })
}

fn node_get_owner_document<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::owner_document(e, h, this)
    })
}

fn node_get_parent_node<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::parent_node(e, h, this)
    })
}

fn node_get_parent_element<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::parent_element(e, h, this)
    })
}

fn node_get_first_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::first_child(e, h, this)
    })
}

fn node_get_last_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::last_child(e, h, this)
    })
}

fn node_get_previous_sibling<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::previous_sibling(e, h, this)
    })
}

fn node_get_next_sibling<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::next_sibling(e, h, this)
    })
}

fn node_get_text_content<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::text_content(e, h, this)
    })
}

fn node_set_text_content<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, value: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        let value = cv::dom_string_or_null(e, h, value)?;
        impls::node::set_text_content(e, h, this, value)
    })
}

fn node_has_child_nodes<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, _args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::has_child_nodes(e, h, this)
    })
}

fn node_contains<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::node_or_null(e, h, cv::arg::<E>(args, 0), &NodeClass::<E>::DEF)?;
        impls::node::contains(e, h, this, a0)
    })
}

fn node_insert_before<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        cv::need(e, args, 2)?;
        let a0 = cv::node(e, h, cv::arg::<E>(args, 0), &NodeClass::<E>::DEF)?;
        let a1 = cv::node_or_null(e, h, cv::arg::<E>(args, 1), &NodeClass::<E>::DEF)?;
        impls::node::insert_before(e, h, this, a0, a1)
    })
}

fn node_append_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::node(e, h, cv::arg::<E>(args, 0), &NodeClass::<E>::DEF)?;
        impls::node::append_child(e, h, this, a0)
    })
}

fn node_remove_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::node(e, h, cv::arg::<E>(args, 0), &NodeClass::<E>::DEF)?;
        impls::node::remove_child(e, h, this, a0)
    })
}

pub struct ElementClass<E>(PhantomData<E>);

impl<E: Engine> ElementClass<E> {
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "Element",
        id: ClassId(3),
        last: ClassId(3),
        parent: Some(&NodeClass::<E>::DEF),
        attributes: &[
            Attribute { name: "localName", get: element_get_local_name::<E>, set: None },
            Attribute { name: "tagName", get: element_get_tag_name::<E>, set: None },
            Attribute { name: "id", get: element_get_id::<E>, set: None },
            Attribute { name: "className", get: element_get_class_name::<E>, set: None },
        ],
        operations: &[
            Operation { name: "getAttribute", length: 1, call: element_get_attribute::<E> },
            Operation { name: "hasAttribute", length: 1, call: element_has_attribute::<E> },
        ],
        constants: &[],
        interceptors: None,
        ops: None,
    };
}

fn element_get_local_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::local_name(e, h, this)
    })
}

fn element_get_tag_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::tag_name(e, h, this)
    })
}

fn element_get_id<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::id(e, h, this)
    })
}

fn element_get_class_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::class_name(e, h, this)
    })
}

fn element_get_attribute<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::element::get_attribute(e, h, this, a0)
    })
}

fn element_has_attribute<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::element::has_attribute(e, h, this, a0)
    })
}

pub struct DocumentClass<E>(PhantomData<E>);

impl<E: Engine> DocumentClass<E> {
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "Document",
        id: ClassId(4),
        last: ClassId(4),
        parent: Some(&NodeClass::<E>::DEF),
        attributes: &[
            Attribute { name: "documentElement", get: document_get_document_element::<E>, set: None },
        ],
        operations: &[
            Operation { name: "createElement", length: 1, call: document_create_element::<E> },
            Operation { name: "createTextNode", length: 1, call: document_create_text_node::<E> },
            Operation { name: "createComment", length: 1, call: document_create_comment::<E> },
            Operation { name: "getElementById", length: 1, call: document_get_element_by_id::<E> },
        ],
        constants: &[],
        interceptors: None,
        ops: None,
    };
}

fn document_get_document_element<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        impls::document::document_element(e, h, this)
    })
}

fn document_create_element<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::create_element(e, h, this, a0)
    })
}

fn document_create_text_node<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::create_text_node(e, h, this, a0)
    })
}

fn document_create_comment<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::create_comment(e, h, this, a0)
    })
}

fn document_get_element_by_id<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::get_element_by_id(e, h, this, a0)
    })
}
