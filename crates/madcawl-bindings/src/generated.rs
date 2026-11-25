// CODEGEN: tools/gen-bindings.py from crates/madcawl-bindings/idl/dom.webidl
//! The class tables of the bound interfaces, and the glue between
//! script values and the implementations in `impls`.

use std::marker::PhantomData;

use madcawl_js::{
    Attribute, AttributeSig, Bound, ClassDef, ClassId, Constant, Ctx, Effects, Engine, IdlType, Operation, Owned,
    Signature,
};

use crate::convert as cv;
use crate::gc::NodeOps;
use crate::host::DomHost;
use crate::impls;
use crate::shell;

pub struct EventTargetClass<E>(PhantomData<E>);

impl<E: Engine> EventTargetClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "EventTarget",
        id: ClassId(1),
        last: ClassId(4),
        parent: None,
        attributes: &[],
        operations: &[
            Operation { name: "addEventListener", length: 2, call: unsafe { Bound::host_fn(event_target_add_event_listener_abi::<E>) }, sig: Signature { args: &[IdlType::String, IdlType::Callback, IdlType::Boolean], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "removeEventListener", length: 2, call: unsafe { Bound::host_fn(event_target_remove_event_listener_abi::<E>) }, sig: Signature { args: &[IdlType::String, IdlType::Callback, IdlType::Boolean], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
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
        let a0 = cv::atom(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::callback_or_null(e, cv::arg::<E>(args, 1))?;
        let a2 = cv::boolean_or(e, cv::arg::<E>(args, 2), false);
        impls::event_target::add_event_listener(e, h, this, a0, a1, a2)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_target_add_event_listener_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, event_target_add_event_listener::<E>) } }

fn event_target_remove_event_listener<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &EventTargetClass::<E>::DEF)?;
        cv::need(e, args, 2)?;
        let a0 = cv::atom(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::callback_or_null(e, cv::arg::<E>(args, 1))?;
        let a2 = cv::boolean_or(e, cv::arg::<E>(args, 2), false);
        impls::event_target::remove_event_listener(e, h, this, a0, a1, a2)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_target_remove_event_listener_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, event_target_remove_event_listener::<E>) } }

pub struct NodeClass<E>(PhantomData<E>);

impl<E: Engine> NodeClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "Node",
        id: ClassId(2),
        last: ClassId(4),
        parent: Some(&EventTargetClass::<E>::DEF),
        attributes: &[
            Attribute { name: "nodeType", get: unsafe { Bound::host_fn(node_get_node_type_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Number, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "nodeName", get: unsafe { Bound::host_fn(node_get_node_name_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::String, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "isConnected", get: unsafe { Bound::host_fn(node_get_is_connected_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Boolean, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "ownerDocument", get: unsafe { Bound::host_fn(node_get_owner_document_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "parentNode", get: unsafe { Bound::host_fn(node_get_parent_node_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "parentElement", get: unsafe { Bound::host_fn(node_get_parent_element_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "firstChild", get: unsafe { Bound::host_fn(node_get_first_child_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "lastChild", get: unsafe { Bound::host_fn(node_get_last_child_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "previousSibling", get: unsafe { Bound::host_fn(node_get_previous_sibling_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "nextSibling", get: unsafe { Bound::host_fn(node_get_next_sibling_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "textContent", get: unsafe { Bound::host_fn(node_get_text_content_abi::<E>) }, set: Some(unsafe { Bound::host_fn(node_set_text_content_abi::<E>) }), sig: AttributeSig { ty: IdlType::NullableString, get: Effects::NONE, set: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::WRITES) } },
        ],
        operations: &[
            Operation { name: "hasChildNodes", length: 0, call: unsafe { Bound::host_fn(node_has_child_nodes_abi::<E>) }, sig: Signature { args: &[], result: IdlType::Boolean, effects: Effects::NONE } },
            Operation { name: "contains", length: 1, call: unsafe { Bound::host_fn(node_contains_abi::<E>) }, sig: Signature { args: &[IdlType::NullableObject], result: IdlType::Boolean, effects: Effects::THROWS } },
            Operation { name: "insertBefore", length: 2, call: unsafe { Bound::host_fn(node_insert_before_abi::<E>) }, sig: Signature { args: &[IdlType::Object, IdlType::NullableObject], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::WRITES) } },
            Operation { name: "appendChild", length: 1, call: unsafe { Bound::host_fn(node_append_child_abi::<E>) }, sig: Signature { args: &[IdlType::Object], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::WRITES) } },
            Operation { name: "removeChild", length: 1, call: unsafe { Bound::host_fn(node_remove_child_abi::<E>) }, sig: Signature { args: &[IdlType::Object], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::WRITES) } },
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

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_get_node_type_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, node_get_node_type::<E>) } }

fn node_get_node_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::node_name(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_get_node_name_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, node_get_node_name::<E>) } }

fn node_get_is_connected<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::is_connected(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_get_is_connected_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, node_get_is_connected::<E>) } }

fn node_get_owner_document<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::owner_document(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_get_owner_document_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, node_get_owner_document::<E>) } }

fn node_get_parent_node<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::parent_node(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_get_parent_node_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, node_get_parent_node::<E>) } }

fn node_get_parent_element<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::parent_element(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_get_parent_element_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, node_get_parent_element::<E>) } }

fn node_get_first_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::first_child(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_get_first_child_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, node_get_first_child::<E>) } }

fn node_get_last_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::last_child(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_get_last_child_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, node_get_last_child::<E>) } }

fn node_get_previous_sibling<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::previous_sibling(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_get_previous_sibling_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, node_get_previous_sibling::<E>) } }

fn node_get_next_sibling<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::next_sibling(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_get_next_sibling_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, node_get_next_sibling::<E>) } }

fn node_get_text_content<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::text_content(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_get_text_content_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, node_get_text_content::<E>) } }

fn node_set_text_content<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, value: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        let value = cv::dom_string_or_null(e, h, value)?;
        impls::node::set_text_content(e, h, this, value)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_set_text_content_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::set(ctx, this, argv, argc, node_set_text_content::<E>) } }

fn node_has_child_nodes<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, _args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::has_child_nodes(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_has_child_nodes_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, node_has_child_nodes::<E>) } }

fn node_contains<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::node_or_null(e, h, cv::arg::<E>(args, 0), &NodeClass::<E>::DEF)?;
        impls::node::contains(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_contains_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, node_contains::<E>) } }

fn node_insert_before<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        cv::need(e, args, 2)?;
        let a0 = cv::node(e, h, cv::arg::<E>(args, 0), &NodeClass::<E>::DEF)?;
        let a1 = cv::node_or_null(e, h, cv::arg::<E>(args, 1), &NodeClass::<E>::DEF)?;
        impls::node::insert_before(e, h, this, a0, a1)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_insert_before_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, node_insert_before::<E>) } }

fn node_append_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::node(e, h, cv::arg::<E>(args, 0), &NodeClass::<E>::DEF)?;
        impls::node::append_child(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_append_child_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, node_append_child::<E>) } }

fn node_remove_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::node(e, h, cv::arg::<E>(args, 0), &NodeClass::<E>::DEF)?;
        impls::node::remove_child(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_remove_child_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, node_remove_child::<E>) } }

pub struct ElementClass<E>(PhantomData<E>);

impl<E: Engine> ElementClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "Element",
        id: ClassId(3),
        last: ClassId(3),
        parent: Some(&NodeClass::<E>::DEF),
        attributes: &[
            Attribute { name: "localName", get: unsafe { Bound::host_fn(element_get_local_name_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::String, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "tagName", get: unsafe { Bound::host_fn(element_get_tag_name_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::String, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "id", get: unsafe { Bound::host_fn(element_get_id_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::String, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "className", get: unsafe { Bound::host_fn(element_get_class_name_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::String, get: Effects::NONE, set: Effects::NONE } },
        ],
        operations: &[
            Operation { name: "getAttribute", length: 1, call: unsafe { Bound::host_fn(element_get_attribute_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::NullableString, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "hasAttribute", length: 1, call: unsafe { Bound::host_fn(element_has_attribute_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Boolean, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
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

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_local_name_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, element_get_local_name::<E>) } }

fn element_get_tag_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::tag_name(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_tag_name_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, element_get_tag_name::<E>) } }

fn element_get_id<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::id(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_id_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, element_get_id::<E>) } }

fn element_get_class_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::class_name(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_class_name_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, element_get_class_name::<E>) } }

fn element_get_attribute<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::element::get_attribute(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_attribute_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, element_get_attribute::<E>) } }

fn element_has_attribute<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::element::has_attribute(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_has_attribute_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, element_has_attribute::<E>) } }

pub struct DocumentClass<E>(PhantomData<E>);

impl<E: Engine> DocumentClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "Document",
        id: ClassId(4),
        last: ClassId(4),
        parent: Some(&NodeClass::<E>::DEF),
        attributes: &[
            Attribute { name: "documentElement", get: unsafe { Bound::host_fn(document_get_document_element_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
        ],
        operations: &[
            Operation { name: "createElement", length: 1, call: unsafe { Bound::host_fn(document_create_element_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::NEW_OBJECT) } },
            Operation { name: "createTextNode", length: 1, call: unsafe { Bound::host_fn(document_create_text_node_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::NEW_OBJECT) } },
            Operation { name: "createComment", length: 1, call: unsafe { Bound::host_fn(document_create_comment_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::NEW_OBJECT) } },
            Operation { name: "getElementById", length: 1, call: unsafe { Bound::host_fn(document_get_element_by_id_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::NullableObject, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
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

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_get_document_element_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, document_get_document_element::<E>) } }

fn document_create_element<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::create_element(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_create_element_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_create_element::<E>) } }

fn document_create_text_node<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::create_text_node(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_create_text_node_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_create_text_node::<E>) } }

fn document_create_comment<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::create_comment(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_create_comment_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_create_comment::<E>) } }

fn document_get_element_by_id<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::get_element_by_id(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_get_element_by_id_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_get_element_by_id::<E>) } }
