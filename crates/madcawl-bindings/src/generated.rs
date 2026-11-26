// CODEGEN: tools/gen-bindings.py from crates/madcawl-bindings/idl/dom.webidl
//! The class tables of the bound interfaces, and the glue between
//! script values and the implementations in `impls`.

use std::marker::PhantomData;

use madcawl_js::{
    Attribute, AttributeSig, Bound, ClassDef, ClassId, Constant, Ctx, Effects, Engine, IdlType, Interceptors,
    Operation, Owned, Signature,
};

use crate::convert as cv;
use crate::gc::NodeOps;
use crate::host::DomHost;
use crate::impls;
use crate::lists;
use crate::shell;
use crate::side_ops::SideOps;

pub struct EventTargetClass<E>(PhantomData<E>);

impl<E: Engine> EventTargetClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "EventTarget",
        id: ClassId(1),
        last: ClassId(8),
        parent: None,
        attributes: &[],
        operations: &[
            Operation { name: "addEventListener", length: 2, call: unsafe { Bound::host_fn(event_target_add_event_listener_abi::<E>) }, sig: Signature { args: &[IdlType::String, IdlType::Callback, IdlType::Any], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "removeEventListener", length: 2, call: unsafe { Bound::host_fn(event_target_remove_event_listener_abi::<E>) }, sig: Signature { args: &[IdlType::String, IdlType::Callback, IdlType::Any], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "dispatchEvent", length: 1, call: unsafe { Bound::host_fn(event_target_dispatch_event_abi::<E>) }, sig: Signature { args: &[IdlType::Object], result: IdlType::Boolean, effects: Effects::THROWS } },
        ],
        constants: &[],
        interceptors: None,
        ops: Some(&SideOps::<E>::OPS),
        constructor: Some(unsafe { Bound::host_fn(event_target_constructor_abi::<E>) }),
        length: 0,
        error_prototype: false,
    };
}

fn event_target_add_event_listener<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_target(e, h, this, &EventTargetClass::<E>::DEF)?;
        cv::need(e, args, 2)?;
        let a0 = cv::atom(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::callback_or_null(e, cv::arg::<E>(args, 1))?;
        let a2 = cv::arg::<E>(args, 2);
        impls::event_target::add_event_listener(e, h, this, a0, a1, a2)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_target_add_event_listener_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, event_target_add_event_listener::<E>) } }

fn event_target_remove_event_listener<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_target(e, h, this, &EventTargetClass::<E>::DEF)?;
        cv::need(e, args, 2)?;
        let a0 = cv::atom(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::callback_or_null(e, cv::arg::<E>(args, 1))?;
        let a2 = cv::arg::<E>(args, 2);
        impls::event_target::remove_event_listener(e, h, this, a0, a1, a2)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_target_remove_event_listener_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, event_target_remove_event_listener::<E>) } }

fn event_target_dispatch_event<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_target(e, h, this, &EventTargetClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::side(e, h, cv::arg::<E>(args, 0), &EventClass::<E>::DEF)?;
        impls::event_target::dispatch_event(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_target_dispatch_event_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, event_target_dispatch_event::<E>) } }

fn event_target_constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: E::Value, _args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        impls::event_target::constructor(e, h)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_target_constructor_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, event_target_constructor::<E>) } }

pub struct NodeClass<E>(PhantomData<E>);

impl<E: Engine> NodeClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "Node",
        id: ClassId(2),
        last: ClassId(8),
        parent: Some(&EventTargetClass::<E>::DEF),
        attributes: &[
            Attribute { name: "nodeType", get: unsafe { Bound::host_fn(node_get_node_type_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Number, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "nodeName", get: unsafe { Bound::host_fn(node_get_node_name_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::String, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "isConnected", get: unsafe { Bound::host_fn(node_get_is_connected_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Boolean, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "ownerDocument", get: unsafe { Bound::host_fn(node_get_owner_document_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "parentNode", get: unsafe { Bound::host_fn(node_get_parent_node_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "parentElement", get: unsafe { Bound::host_fn(node_get_parent_element_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "childNodes", get: unsafe { Bound::host_fn(node_get_child_nodes_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Object, get: Effects::NONE, set: Effects::NONE } },
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
        constructor: None,
        length: 0,
        error_prototype: false,
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

fn node_get_child_nodes<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &NodeClass::<E>::DEF)?;
        impls::node::child_nodes(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_get_child_nodes_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, node_get_child_nodes::<E>) } }

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
            Attribute { name: "id", get: unsafe { Bound::host_fn(element_get_id_abi::<E>) }, set: Some(unsafe { Bound::host_fn(element_set_id_abi::<E>) }), sig: AttributeSig { ty: IdlType::String, get: Effects::NONE, set: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::WRITES) } },
            Attribute { name: "className", get: unsafe { Bound::host_fn(element_get_class_name_abi::<E>) }, set: Some(unsafe { Bound::host_fn(element_set_class_name_abi::<E>) }), sig: AttributeSig { ty: IdlType::String, get: Effects::NONE, set: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::WRITES) } },
            Attribute { name: "previousElementSibling", get: unsafe { Bound::host_fn(element_get_previous_element_sibling_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "nextElementSibling", get: unsafe { Bound::host_fn(element_get_next_element_sibling_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "children", get: unsafe { Bound::host_fn(element_get_children_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Object, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "firstElementChild", get: unsafe { Bound::host_fn(element_get_first_element_child_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "lastElementChild", get: unsafe { Bound::host_fn(element_get_last_element_child_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "childElementCount", get: unsafe { Bound::host_fn(element_get_child_element_count_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Number, get: Effects::NONE, set: Effects::NONE } },
        ],
        operations: &[
            Operation { name: "hasAttributes", length: 0, call: unsafe { Bound::host_fn(element_has_attributes_abi::<E>) }, sig: Signature { args: &[], result: IdlType::Boolean, effects: Effects::NONE } },
            Operation { name: "getAttributeNames", length: 0, call: unsafe { Bound::host_fn(element_get_attribute_names_abi::<E>) }, sig: Signature { args: &[], result: IdlType::Object, effects: Effects::NONE } },
            Operation { name: "getAttribute", length: 1, call: unsafe { Bound::host_fn(element_get_attribute_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::NullableString, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "setAttribute", length: 2, call: unsafe { Bound::host_fn(element_set_attribute_abi::<E>) }, sig: Signature { args: &[IdlType::String, IdlType::String], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::WRITES) } },
            Operation { name: "removeAttribute", length: 1, call: unsafe { Bound::host_fn(element_remove_attribute_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::WRITES) } },
            Operation { name: "toggleAttribute", length: 1, call: unsafe { Bound::host_fn(element_toggle_attribute_abi::<E>) }, sig: Signature { args: &[IdlType::String, IdlType::Any], result: IdlType::Boolean, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::WRITES) } },
            Operation { name: "hasAttribute", length: 1, call: unsafe { Bound::host_fn(element_has_attribute_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Boolean, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "getElementsByTagName", length: 1, call: unsafe { Bound::host_fn(element_get_elements_by_tag_name_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "getElementsByClassName", length: 1, call: unsafe { Bound::host_fn(element_get_elements_by_class_name_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "remove", length: 0, call: unsafe { Bound::host_fn(element_remove_abi::<E>) }, sig: Signature { args: &[], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::WRITES) } },
            Operation { name: "querySelector", length: 1, call: unsafe { Bound::host_fn(element_query_selector_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::NullableObject, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "querySelectorAll", length: 1, call: unsafe { Bound::host_fn(element_query_selector_all_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::NEW_OBJECT) } },
        ],
        constants: &[],
        interceptors: None,
        ops: None,
        constructor: None,
        length: 0,
        error_prototype: false,
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

fn element_set_id<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, value: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        let value = cv::dom_string(e, h, value)?;
        impls::element::set_id(e, h, this, value)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_set_id_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::set(ctx, this, argv, argc, element_set_id::<E>) } }

fn element_get_class_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::class_name(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_class_name_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, element_get_class_name::<E>) } }

fn element_set_class_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, value: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        let value = cv::dom_string(e, h, value)?;
        impls::element::set_class_name(e, h, this, value)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_set_class_name_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::set(ctx, this, argv, argc, element_set_class_name::<E>) } }

fn element_get_previous_element_sibling<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::previous_element_sibling(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_previous_element_sibling_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, element_get_previous_element_sibling::<E>) } }

fn element_get_next_element_sibling<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::next_element_sibling(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_next_element_sibling_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, element_get_next_element_sibling::<E>) } }

fn element_get_children<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::children(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_children_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, element_get_children::<E>) } }

fn element_get_first_element_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::first_element_child(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_first_element_child_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, element_get_first_element_child::<E>) } }

fn element_get_last_element_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::last_element_child(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_last_element_child_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, element_get_last_element_child::<E>) } }

fn element_get_child_element_count<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::child_element_count(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_child_element_count_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, element_get_child_element_count::<E>) } }

fn element_has_attributes<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, _args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::has_attributes(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_has_attributes_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, element_has_attributes::<E>) } }

fn element_get_attribute_names<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, _args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::get_attribute_names(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_attribute_names_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, element_get_attribute_names::<E>) } }

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

fn element_set_attribute<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        cv::need(e, args, 2)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::dom_string(e, h, cv::arg::<E>(args, 1))?;
        impls::element::set_attribute(e, h, this, a0, a1)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_set_attribute_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, element_set_attribute::<E>) } }

fn element_remove_attribute<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::element::remove_attribute(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_remove_attribute_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, element_remove_attribute::<E>) } }

fn element_toggle_attribute<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::arg::<E>(args, 1);
        impls::element::toggle_attribute(e, h, this, a0, a1)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_toggle_attribute_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, element_toggle_attribute::<E>) } }

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

fn element_get_elements_by_tag_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::element::get_elements_by_tag_name(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_elements_by_tag_name_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, element_get_elements_by_tag_name::<E>) } }

fn element_get_elements_by_class_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::element::get_elements_by_class_name(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_get_elements_by_class_name_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, element_get_elements_by_class_name::<E>) } }

fn element_remove<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, _args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        impls::element::remove(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_remove_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, element_remove::<E>) } }

fn element_query_selector<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::element::query_selector(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_query_selector_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, element_query_selector::<E>) } }

fn element_query_selector_all<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &ElementClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::element::query_selector_all(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn element_query_selector_all_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, element_query_selector_all::<E>) } }

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
            Attribute { name: "children", get: unsafe { Bound::host_fn(document_get_children_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Object, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "firstElementChild", get: unsafe { Bound::host_fn(document_get_first_element_child_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "lastElementChild", get: unsafe { Bound::host_fn(document_get_last_element_child_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "childElementCount", get: unsafe { Bound::host_fn(document_get_child_element_count_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Number, get: Effects::NONE, set: Effects::NONE } },
        ],
        operations: &[
            Operation { name: "createElement", length: 1, call: unsafe { Bound::host_fn(document_create_element_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::NEW_OBJECT) } },
            Operation { name: "createDocumentFragment", length: 0, call: unsafe { Bound::host_fn(document_create_document_fragment_abi::<E>) }, sig: Signature { args: &[], result: IdlType::Object, effects: Effects::THROWS.with(Effects::NEW_OBJECT) } },
            Operation { name: "createTextNode", length: 1, call: unsafe { Bound::host_fn(document_create_text_node_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::NEW_OBJECT) } },
            Operation { name: "createComment", length: 1, call: unsafe { Bound::host_fn(document_create_comment_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::NEW_OBJECT) } },
            Operation { name: "getElementById", length: 1, call: unsafe { Bound::host_fn(document_get_element_by_id_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::NullableObject, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "getElementsByTagName", length: 1, call: unsafe { Bound::host_fn(document_get_elements_by_tag_name_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "getElementsByClassName", length: 1, call: unsafe { Bound::host_fn(document_get_elements_by_class_name_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "createEvent", length: 1, call: unsafe { Bound::host_fn(document_create_event_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::NEW_OBJECT) } },
            Operation { name: "querySelector", length: 1, call: unsafe { Bound::host_fn(document_query_selector_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::NullableObject, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "querySelectorAll", length: 1, call: unsafe { Bound::host_fn(document_query_selector_all_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::NEW_OBJECT) } },
            Operation { name: "write", length: 1, call: unsafe { Bound::host_fn(document_write_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::WRITES) } },
        ],
        constants: &[],
        interceptors: None,
        ops: None,
        constructor: None,
        length: 0,
        error_prototype: false,
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

fn document_get_children<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        impls::document::children(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_get_children_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, document_get_children::<E>) } }

fn document_get_first_element_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        impls::document::first_element_child(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_get_first_element_child_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, document_get_first_element_child::<E>) } }

fn document_get_last_element_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        impls::document::last_element_child(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_get_last_element_child_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, document_get_last_element_child::<E>) } }

fn document_get_child_element_count<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        impls::document::child_element_count(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_get_child_element_count_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, document_get_child_element_count::<E>) } }

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

fn document_create_document_fragment<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, _args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        impls::document::create_document_fragment(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_create_document_fragment_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_create_document_fragment::<E>) } }

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

fn document_get_elements_by_tag_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::get_elements_by_tag_name(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_get_elements_by_tag_name_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_get_elements_by_tag_name::<E>) } }

fn document_get_elements_by_class_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::get_elements_by_class_name(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_get_elements_by_class_name_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_get_elements_by_class_name::<E>) } }

fn document_create_event<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::create_event(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_create_event_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_create_event::<E>) } }

fn document_query_selector<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::query_selector(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_query_selector_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_query_selector::<E>) } }

fn document_query_selector_all<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::query_selector_all(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_query_selector_all_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_query_selector_all::<E>) } }

fn document_write<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document::write(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_write_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_write::<E>) } }

pub struct DocumentFragmentClass<E>(PhantomData<E>);

impl<E: Engine> DocumentFragmentClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "DocumentFragment",
        id: ClassId(5),
        last: ClassId(5),
        parent: Some(&NodeClass::<E>::DEF),
        attributes: &[
            Attribute { name: "children", get: unsafe { Bound::host_fn(document_fragment_get_children_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Object, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "firstElementChild", get: unsafe { Bound::host_fn(document_fragment_get_first_element_child_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "lastElementChild", get: unsafe { Bound::host_fn(document_fragment_get_last_element_child_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "childElementCount", get: unsafe { Bound::host_fn(document_fragment_get_child_element_count_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Number, get: Effects::NONE, set: Effects::NONE } },
        ],
        operations: &[
            Operation { name: "getElementById", length: 1, call: unsafe { Bound::host_fn(document_fragment_get_element_by_id_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::NullableObject, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "querySelector", length: 1, call: unsafe { Bound::host_fn(document_fragment_query_selector_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::NullableObject, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "querySelectorAll", length: 1, call: unsafe { Bound::host_fn(document_fragment_query_selector_all_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Object, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::NEW_OBJECT) } },
        ],
        constants: &[],
        interceptors: None,
        ops: None,
        constructor: Some(unsafe { Bound::host_fn(document_fragment_constructor_abi::<E>) }),
        length: 0,
        error_prototype: false,
    };
}

fn document_fragment_get_children<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentFragmentClass::<E>::DEF)?;
        impls::document_fragment::children(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_fragment_get_children_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, document_fragment_get_children::<E>) } }

fn document_fragment_get_first_element_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentFragmentClass::<E>::DEF)?;
        impls::document_fragment::first_element_child(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_fragment_get_first_element_child_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, document_fragment_get_first_element_child::<E>) } }

fn document_fragment_get_last_element_child<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentFragmentClass::<E>::DEF)?;
        impls::document_fragment::last_element_child(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_fragment_get_last_element_child_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, document_fragment_get_last_element_child::<E>) } }

fn document_fragment_get_child_element_count<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentFragmentClass::<E>::DEF)?;
        impls::document_fragment::child_element_count(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_fragment_get_child_element_count_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, document_fragment_get_child_element_count::<E>) } }

fn document_fragment_get_element_by_id<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentFragmentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document_fragment::get_element_by_id(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_fragment_get_element_by_id_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_fragment_get_element_by_id::<E>) } }

fn document_fragment_query_selector<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentFragmentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document_fragment::query_selector(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_fragment_query_selector_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_fragment_query_selector::<E>) } }

fn document_fragment_query_selector_all<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &DocumentFragmentClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::document_fragment::query_selector_all(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_fragment_query_selector_all_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_fragment_query_selector_all::<E>) } }

fn document_fragment_constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: E::Value, _args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        impls::document_fragment::constructor(e, h)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn document_fragment_constructor_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, document_fragment_constructor::<E>) } }

pub struct CharacterDataClass<E>(PhantomData<E>);

impl<E: Engine> CharacterDataClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "CharacterData",
        id: ClassId(6),
        last: ClassId(8),
        parent: Some(&NodeClass::<E>::DEF),
        attributes: &[
            Attribute { name: "data", get: unsafe { Bound::host_fn(character_data_get_data_abi::<E>) }, set: Some(unsafe { Bound::host_fn(character_data_set_data_abi::<E>) }), sig: AttributeSig { ty: IdlType::String, get: Effects::NONE, set: Effects::RUNS_SCRIPT.with(Effects::THROWS).with(Effects::WRITES) } },
            Attribute { name: "length", get: unsafe { Bound::host_fn(character_data_get_length_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Number, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "previousElementSibling", get: unsafe { Bound::host_fn(character_data_get_previous_element_sibling_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "nextElementSibling", get: unsafe { Bound::host_fn(character_data_get_next_element_sibling_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
        ],
        operations: &[
            Operation { name: "substringData", length: 2, call: unsafe { Bound::host_fn(character_data_substring_data_abi::<E>) }, sig: Signature { args: &[IdlType::Number, IdlType::Number], result: IdlType::String, effects: Effects::THROWS } },
            Operation { name: "appendData", length: 1, call: unsafe { Bound::host_fn(character_data_append_data_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "insertData", length: 2, call: unsafe { Bound::host_fn(character_data_insert_data_abi::<E>) }, sig: Signature { args: &[IdlType::Number, IdlType::String], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "deleteData", length: 2, call: unsafe { Bound::host_fn(character_data_delete_data_abi::<E>) }, sig: Signature { args: &[IdlType::Number, IdlType::Number], result: IdlType::Undefined, effects: Effects::THROWS } },
            Operation { name: "replaceData", length: 3, call: unsafe { Bound::host_fn(character_data_replace_data_abi::<E>) }, sig: Signature { args: &[IdlType::Number, IdlType::Number, IdlType::String], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
            Operation { name: "remove", length: 0, call: unsafe { Bound::host_fn(character_data_remove_abi::<E>) }, sig: Signature { args: &[], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::WRITES) } },
        ],
        constants: &[],
        interceptors: None,
        ops: None,
        constructor: None,
        length: 0,
        error_prototype: false,
    };
}

fn character_data_get_data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &CharacterDataClass::<E>::DEF)?;
        impls::character_data::data(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn character_data_get_data_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, character_data_get_data::<E>) } }

fn character_data_set_data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, value: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &CharacterDataClass::<E>::DEF)?;
        let value = cv::dom_string_null_empty(e, h, value)?;
        impls::character_data::set_data(e, h, this, value)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn character_data_set_data_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::set(ctx, this, argv, argc, character_data_set_data::<E>) } }

fn character_data_get_length<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &CharacterDataClass::<E>::DEF)?;
        impls::character_data::length(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn character_data_get_length_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, character_data_get_length::<E>) } }

fn character_data_get_previous_element_sibling<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &CharacterDataClass::<E>::DEF)?;
        impls::character_data::previous_element_sibling(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn character_data_get_previous_element_sibling_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, character_data_get_previous_element_sibling::<E>) } }

fn character_data_get_next_element_sibling<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &CharacterDataClass::<E>::DEF)?;
        impls::character_data::next_element_sibling(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn character_data_get_next_element_sibling_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, character_data_get_next_element_sibling::<E>) } }

fn character_data_substring_data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &CharacterDataClass::<E>::DEF)?;
        cv::need(e, args, 2)?;
        let a0 = cv::to_uint32(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::to_uint32(e, h, cv::arg::<E>(args, 1))?;
        impls::character_data::substring_data(e, h, this, a0, a1)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn character_data_substring_data_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, character_data_substring_data::<E>) } }

fn character_data_append_data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &CharacterDataClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::character_data::append_data(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn character_data_append_data_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, character_data_append_data::<E>) } }

fn character_data_insert_data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &CharacterDataClass::<E>::DEF)?;
        cv::need(e, args, 2)?;
        let a0 = cv::to_uint32(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::dom_string(e, h, cv::arg::<E>(args, 1))?;
        impls::character_data::insert_data(e, h, this, a0, a1)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn character_data_insert_data_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, character_data_insert_data::<E>) } }

fn character_data_delete_data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &CharacterDataClass::<E>::DEF)?;
        cv::need(e, args, 2)?;
        let a0 = cv::to_uint32(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::to_uint32(e, h, cv::arg::<E>(args, 1))?;
        impls::character_data::delete_data(e, h, this, a0, a1)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn character_data_delete_data_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, character_data_delete_data::<E>) } }

fn character_data_replace_data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &CharacterDataClass::<E>::DEF)?;
        cv::need(e, args, 3)?;
        let a0 = cv::to_uint32(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::to_uint32(e, h, cv::arg::<E>(args, 1))?;
        let a2 = cv::dom_string(e, h, cv::arg::<E>(args, 2))?;
        impls::character_data::replace_data(e, h, this, a0, a1, a2)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn character_data_replace_data_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, character_data_replace_data::<E>) } }

fn character_data_remove<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, _args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &CharacterDataClass::<E>::DEF)?;
        impls::character_data::remove(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn character_data_remove_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, character_data_remove::<E>) } }

pub struct TextClass<E>(PhantomData<E>);

impl<E: Engine> TextClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "Text",
        id: ClassId(7),
        last: ClassId(7),
        parent: Some(&CharacterDataClass::<E>::DEF),
        attributes: &[
            Attribute { name: "wholeText", get: unsafe { Bound::host_fn(text_get_whole_text_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::String, get: Effects::NONE, set: Effects::NONE } },
        ],
        operations: &[
            Operation { name: "splitText", length: 1, call: unsafe { Bound::host_fn(text_split_text_abi::<E>) }, sig: Signature { args: &[IdlType::Number], result: IdlType::Object, effects: Effects::THROWS.with(Effects::NEW_OBJECT) } },
        ],
        constants: &[],
        interceptors: None,
        ops: None,
        constructor: Some(unsafe { Bound::host_fn(text_constructor_abi::<E>) }),
        length: 0,
        error_prototype: false,
    };
}

fn text_get_whole_text<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &TextClass::<E>::DEF)?;
        impls::text::whole_text(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn text_get_whole_text_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, text_get_whole_text::<E>) } }

fn text_split_text<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_node(e, h, this, &TextClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::to_uint32(e, h, cv::arg::<E>(args, 0))?;
        impls::text::split_text(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn text_split_text_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, text_split_text::<E>) } }

fn text_constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let a0 = cv::dom_string_or(e, h, cv::arg::<E>(args, 0), b"")?;
        impls::text::constructor(e, h, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn text_constructor_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, text_constructor::<E>) } }

pub struct CommentClass<E>(PhantomData<E>);

impl<E: Engine> CommentClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "Comment",
        id: ClassId(8),
        last: ClassId(8),
        parent: Some(&CharacterDataClass::<E>::DEF),
        attributes: &[],
        operations: &[],
        constants: &[],
        interceptors: None,
        ops: None,
        constructor: Some(unsafe { Bound::host_fn(comment_constructor_abi::<E>) }),
        length: 0,
        error_prototype: false,
    };
}

fn comment_constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let a0 = cv::dom_string_or(e, h, cv::arg::<E>(args, 0), b"")?;
        impls::comment::constructor(e, h, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn comment_constructor_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, comment_constructor::<E>) } }

pub struct DOMExceptionClass<E>(PhantomData<E>);

impl<E: Engine> DOMExceptionClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "DOMException",
        id: ClassId(9),
        last: ClassId(9),
        parent: None,
        attributes: &[
            Attribute { name: "name", get: unsafe { Bound::host_fn(dom_exception_get_name_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::String, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "message", get: unsafe { Bound::host_fn(dom_exception_get_message_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::String, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "code", get: unsafe { Bound::host_fn(dom_exception_get_code_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Number, get: Effects::NONE, set: Effects::NONE } },
        ],
        operations: &[],
        constants: &[
            Constant { name: "INDEX_SIZE_ERR", value: 1.0 },
            Constant { name: "DOMSTRING_SIZE_ERR", value: 2.0 },
            Constant { name: "HIERARCHY_REQUEST_ERR", value: 3.0 },
            Constant { name: "WRONG_DOCUMENT_ERR", value: 4.0 },
            Constant { name: "INVALID_CHARACTER_ERR", value: 5.0 },
            Constant { name: "NO_DATA_ALLOWED_ERR", value: 6.0 },
            Constant { name: "NO_MODIFICATION_ALLOWED_ERR", value: 7.0 },
            Constant { name: "NOT_FOUND_ERR", value: 8.0 },
            Constant { name: "NOT_SUPPORTED_ERR", value: 9.0 },
            Constant { name: "INUSE_ATTRIBUTE_ERR", value: 10.0 },
            Constant { name: "INVALID_STATE_ERR", value: 11.0 },
            Constant { name: "SYNTAX_ERR", value: 12.0 },
            Constant { name: "INVALID_MODIFICATION_ERR", value: 13.0 },
            Constant { name: "NAMESPACE_ERR", value: 14.0 },
            Constant { name: "INVALID_ACCESS_ERR", value: 15.0 },
            Constant { name: "VALIDATION_ERR", value: 16.0 },
            Constant { name: "TYPE_MISMATCH_ERR", value: 17.0 },
            Constant { name: "SECURITY_ERR", value: 18.0 },
            Constant { name: "NETWORK_ERR", value: 19.0 },
            Constant { name: "ABORT_ERR", value: 20.0 },
            Constant { name: "URL_MISMATCH_ERR", value: 21.0 },
            Constant { name: "QUOTA_EXCEEDED_ERR", value: 22.0 },
            Constant { name: "TIMEOUT_ERR", value: 23.0 },
            Constant { name: "INVALID_NODE_TYPE_ERR", value: 24.0 },
            Constant { name: "DATA_CLONE_ERR", value: 25.0 },
        ],
        interceptors: None,
        ops: Some(&SideOps::<E>::OPS),
        constructor: Some(unsafe { Bound::host_fn(dom_exception_constructor_abi::<E>) }),
        length: 0,
        error_prototype: true,
    };
}

fn dom_exception_get_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &DOMExceptionClass::<E>::DEF)?;
        impls::dom_exception::name(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn dom_exception_get_name_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, dom_exception_get_name::<E>) } }

fn dom_exception_get_message<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &DOMExceptionClass::<E>::DEF)?;
        impls::dom_exception::message(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn dom_exception_get_message_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, dom_exception_get_message::<E>) } }

fn dom_exception_get_code<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &DOMExceptionClass::<E>::DEF)?;
        impls::dom_exception::code(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn dom_exception_get_code_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, dom_exception_get_code::<E>) } }

fn dom_exception_constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let a0 = cv::dom_string_or(e, h, cv::arg::<E>(args, 0), b"")?;
        let a1 = cv::dom_string_or(e, h, cv::arg::<E>(args, 1), b"Error")?;
        impls::dom_exception::constructor(e, h, a0, a1)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn dom_exception_constructor_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, dom_exception_constructor::<E>) } }

pub struct EventClass<E>(PhantomData<E>);

impl<E: Engine> EventClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "Event",
        id: ClassId(10),
        last: ClassId(11),
        parent: None,
        attributes: &[
            Attribute { name: "type", get: unsafe { Bound::host_fn(event_get_type_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::String, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "target", get: unsafe { Bound::host_fn(event_get_target_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "srcElement", get: unsafe { Bound::host_fn(event_get_src_element_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "currentTarget", get: unsafe { Bound::host_fn(event_get_current_target_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::NullableObject, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "eventPhase", get: unsafe { Bound::host_fn(event_get_event_phase_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Number, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "cancelBubble", get: unsafe { Bound::host_fn(event_get_cancel_bubble_abi::<E>) }, set: Some(unsafe { Bound::host_fn(event_set_cancel_bubble_abi::<E>) }), sig: AttributeSig { ty: IdlType::Boolean, get: Effects::NONE, set: Effects::THROWS.with(Effects::WRITES) } },
            Attribute { name: "bubbles", get: unsafe { Bound::host_fn(event_get_bubbles_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Boolean, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "cancelable", get: unsafe { Bound::host_fn(event_get_cancelable_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Boolean, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "returnValue", get: unsafe { Bound::host_fn(event_get_return_value_abi::<E>) }, set: Some(unsafe { Bound::host_fn(event_set_return_value_abi::<E>) }), sig: AttributeSig { ty: IdlType::Boolean, get: Effects::NONE, set: Effects::THROWS.with(Effects::WRITES) } },
            Attribute { name: "defaultPrevented", get: unsafe { Bound::host_fn(event_get_default_prevented_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Boolean, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "composed", get: unsafe { Bound::host_fn(event_get_composed_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Boolean, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "isTrusted", get: unsafe { Bound::host_fn(event_get_is_trusted_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Boolean, get: Effects::NONE, set: Effects::NONE } },
            Attribute { name: "timeStamp", get: unsafe { Bound::host_fn(event_get_time_stamp_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Number, get: Effects::NONE, set: Effects::NONE } },
        ],
        operations: &[
            Operation { name: "composedPath", length: 0, call: unsafe { Bound::host_fn(event_composed_path_abi::<E>) }, sig: Signature { args: &[], result: IdlType::Object, effects: Effects::NONE } },
            Operation { name: "stopPropagation", length: 0, call: unsafe { Bound::host_fn(event_stop_propagation_abi::<E>) }, sig: Signature { args: &[], result: IdlType::Undefined, effects: Effects::NONE } },
            Operation { name: "stopImmediatePropagation", length: 0, call: unsafe { Bound::host_fn(event_stop_immediate_propagation_abi::<E>) }, sig: Signature { args: &[], result: IdlType::Undefined, effects: Effects::NONE } },
            Operation { name: "preventDefault", length: 0, call: unsafe { Bound::host_fn(event_prevent_default_abi::<E>) }, sig: Signature { args: &[], result: IdlType::Undefined, effects: Effects::NONE } },
            Operation { name: "initEvent", length: 1, call: unsafe { Bound::host_fn(event_init_event_abi::<E>) }, sig: Signature { args: &[IdlType::String, IdlType::Boolean, IdlType::Boolean], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
        ],
        constants: &[
            Constant { name: "NONE", value: 0.0 },
            Constant { name: "CAPTURING_PHASE", value: 1.0 },
            Constant { name: "AT_TARGET", value: 2.0 },
            Constant { name: "BUBBLING_PHASE", value: 3.0 },
        ],
        interceptors: None,
        ops: Some(&SideOps::<E>::OPS),
        constructor: Some(unsafe { Bound::host_fn(event_constructor_abi::<E>) }),
        length: 1,
        error_prototype: false,
    };
}

fn event_get_type<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::r#type(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_get_type_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, event_get_type::<E>) } }

fn event_get_target<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::target(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_get_target_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, event_get_target::<E>) } }

fn event_get_src_element<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::src_element(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_get_src_element_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, event_get_src_element::<E>) } }

fn event_get_current_target<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::current_target(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_get_current_target_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, event_get_current_target::<E>) } }

fn event_get_event_phase<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::event_phase(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_get_event_phase_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, event_get_event_phase::<E>) } }

fn event_get_cancel_bubble<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::cancel_bubble(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_get_cancel_bubble_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, event_get_cancel_bubble::<E>) } }

fn event_set_cancel_bubble<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, value: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        let value = e.to_boolean(value);
        impls::event::set_cancel_bubble(e, h, this, value)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_set_cancel_bubble_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::set(ctx, this, argv, argc, event_set_cancel_bubble::<E>) } }

fn event_get_bubbles<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::bubbles(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_get_bubbles_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, event_get_bubbles::<E>) } }

fn event_get_cancelable<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::cancelable(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_get_cancelable_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, event_get_cancelable::<E>) } }

fn event_get_return_value<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::return_value(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_get_return_value_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, event_get_return_value::<E>) } }

fn event_set_return_value<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, value: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        let value = e.to_boolean(value);
        impls::event::set_return_value(e, h, this, value)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_set_return_value_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::set(ctx, this, argv, argc, event_set_return_value::<E>) } }

fn event_get_default_prevented<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::default_prevented(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_get_default_prevented_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, event_get_default_prevented::<E>) } }

fn event_get_composed<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::composed(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_get_composed_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, event_get_composed::<E>) } }

fn event_get_is_trusted<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::is_trusted(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_get_is_trusted_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, event_get_is_trusted::<E>) } }

fn event_get_time_stamp<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::time_stamp(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_get_time_stamp_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, event_get_time_stamp::<E>) } }

fn event_composed_path<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, _args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::composed_path(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_composed_path_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, event_composed_path::<E>) } }

fn event_stop_propagation<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, _args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::stop_propagation(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_stop_propagation_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, event_stop_propagation::<E>) } }

fn event_stop_immediate_propagation<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, _args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::stop_immediate_propagation(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_stop_immediate_propagation_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, event_stop_immediate_propagation::<E>) } }

fn event_prevent_default<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, _args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        impls::event::prevent_default(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_prevent_default_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, event_prevent_default::<E>) } }

fn event_init_event<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &EventClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::boolean_or(e, cv::arg::<E>(args, 1), false);
        let a2 = cv::boolean_or(e, cv::arg::<E>(args, 2), false);
        impls::event::init_event(e, h, this, a0, a1, a2)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_init_event_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, event_init_event::<E>) } }

fn event_constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::arg::<E>(args, 1);
        impls::event::constructor(e, h, a0, a1)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn event_constructor_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, event_constructor::<E>) } }

pub struct CustomEventClass<E>(PhantomData<E>);

impl<E: Engine> CustomEventClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "CustomEvent",
        id: ClassId(11),
        last: ClassId(11),
        parent: Some(&EventClass::<E>::DEF),
        attributes: &[
            Attribute { name: "detail", get: unsafe { Bound::host_fn(custom_event_get_detail_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Any, get: Effects::NONE, set: Effects::NONE } },
        ],
        operations: &[
            Operation { name: "initCustomEvent", length: 1, call: unsafe { Bound::host_fn(custom_event_init_custom_event_abi::<E>) }, sig: Signature { args: &[IdlType::String, IdlType::Boolean, IdlType::Boolean, IdlType::Any], result: IdlType::Undefined, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
        ],
        constants: &[],
        interceptors: None,
        ops: Some(&SideOps::<E>::OPS),
        constructor: Some(unsafe { Bound::host_fn(custom_event_constructor_abi::<E>) }),
        length: 1,
        error_prototype: false,
    };
}

fn custom_event_get_detail<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &CustomEventClass::<E>::DEF)?;
        impls::custom_event::detail(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn custom_event_get_detail_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, custom_event_get_detail::<E>) } }

fn custom_event_init_custom_event<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &CustomEventClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::boolean_or(e, cv::arg::<E>(args, 1), false);
        let a2 = cv::boolean_or(e, cv::arg::<E>(args, 2), false);
        let a3 = cv::any_or_null::<E>(e, cv::arg::<E>(args, 3));
        impls::custom_event::init_custom_event(e, h, this, a0, a1, a2, a3)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn custom_event_init_custom_event_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, custom_event_init_custom_event::<E>) } }

fn custom_event_constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, _this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        let a1 = cv::arg::<E>(args, 1);
        impls::custom_event::constructor(e, h, a0, a1)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn custom_event_constructor_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, custom_event_constructor::<E>) } }

pub struct NodeListClass<E>(PhantomData<E>);

impl<E: Engine> NodeListClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "NodeList",
        id: ClassId(12),
        last: ClassId(12),
        parent: None,
        attributes: &[
            Attribute { name: "length", get: unsafe { Bound::host_fn(node_list_get_length_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Number, get: Effects::NONE, set: Effects::NONE } },
        ],
        operations: &[
            Operation { name: "item", length: 1, call: unsafe { Bound::host_fn(node_list_item_abi::<E>) }, sig: Signature { args: &[IdlType::Number], result: IdlType::NullableObject, effects: Effects::THROWS } },
        ],
        constants: &[],
        interceptors: Some(&Self::INTERCEPTORS),
        ops: Some(&SideOps::<E>::OPS),
        constructor: None,
        length: 0,
        error_prototype: false,
    };

    // SAFETY: as for DEF
    const INTERCEPTORS: Interceptors<E, DomHost<E>> = Interceptors {
        index: Some(unsafe { Bound::index(node_list_index_abi::<E>) }),
        length: Some(unsafe { Bound::length(node_list_count_abi::<E>) }),
        iterable: true,
    };
}

fn node_list_index<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, index: u32) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &NodeListClass::<E>::DEF)?;
        impls::node_list::item(e, h, this, index).map(lists::OrAbsent)
    })
}

// SAFETY: called as the index getter of its table
unsafe extern "C" fn node_list_index_abi<E: Engine>(ctx: *mut Ctx, this: u64, index: u32) -> u64 { unsafe { shell::index(ctx, this, index, node_list_index::<E>) } }

fn node_list_count<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> u32 {
    lists::count(e, h, this, &NodeListClass::<E>::DEF, impls::node_list::length)
}

// SAFETY: called as the length of its table
unsafe extern "C" fn node_list_count_abi<E: Engine>(ctx: *mut Ctx, this: u64) -> u32 { unsafe { shell::count(ctx, this, node_list_count::<E>) } }

fn node_list_get_length<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &NodeListClass::<E>::DEF)?;
        impls::node_list::length(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_list_get_length_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, node_list_get_length::<E>) } }

fn node_list_item<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &NodeListClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::to_uint32(e, h, cv::arg::<E>(args, 0))?;
        impls::node_list::item(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn node_list_item_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, node_list_item::<E>) } }

pub struct HTMLCollectionClass<E>(PhantomData<E>);

impl<E: Engine> HTMLCollectionClass<E> {
    // SAFETY: every shell in the table reads its context as E and DomHost<E>
    pub const DEF: ClassDef<E, DomHost<E>> = ClassDef {
        name: "HTMLCollection",
        id: ClassId(13),
        last: ClassId(13),
        parent: None,
        attributes: &[
            Attribute { name: "length", get: unsafe { Bound::host_fn(html_collection_get_length_abi::<E>) }, set: None, sig: AttributeSig { ty: IdlType::Number, get: Effects::NONE, set: Effects::NONE } },
        ],
        operations: &[
            Operation { name: "item", length: 1, call: unsafe { Bound::host_fn(html_collection_item_abi::<E>) }, sig: Signature { args: &[IdlType::Number], result: IdlType::NullableObject, effects: Effects::THROWS } },
            Operation { name: "namedItem", length: 1, call: unsafe { Bound::host_fn(html_collection_named_item_abi::<E>) }, sig: Signature { args: &[IdlType::String], result: IdlType::NullableObject, effects: Effects::RUNS_SCRIPT.with(Effects::THROWS) } },
        ],
        constants: &[],
        interceptors: Some(&Self::INTERCEPTORS),
        ops: Some(&SideOps::<E>::OPS),
        constructor: None,
        length: 0,
        error_prototype: false,
    };

    // SAFETY: as for DEF
    const INTERCEPTORS: Interceptors<E, DomHost<E>> = Interceptors {
        index: Some(unsafe { Bound::index(html_collection_index_abi::<E>) }),
        length: Some(unsafe { Bound::length(html_collection_count_abi::<E>) }),
        iterable: false,
    };
}

fn html_collection_index<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, index: u32) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &HTMLCollectionClass::<E>::DEF)?;
        impls::html_collection::item(e, h, this, index).map(lists::OrAbsent)
    })
}

// SAFETY: called as the index getter of its table
unsafe extern "C" fn html_collection_index_abi<E: Engine>(ctx: *mut Ctx, this: u64, index: u32) -> u64 { unsafe { shell::index(ctx, this, index, html_collection_index::<E>) } }

fn html_collection_count<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> u32 {
    lists::count(e, h, this, &HTMLCollectionClass::<E>::DEF, impls::html_collection::length)
}

// SAFETY: called as the length of its table
unsafe extern "C" fn html_collection_count_abi<E: Engine>(ctx: *mut Ctx, this: u64) -> u32 { unsafe { shell::count(ctx, this, html_collection_count::<E>) } }

fn html_collection_get_length<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &HTMLCollectionClass::<E>::DEF)?;
        impls::html_collection::length(e, h, this)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn html_collection_get_length_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 { unsafe { shell::get(ctx, this, html_collection_get_length::<E>) } }

fn html_collection_item<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &HTMLCollectionClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::to_uint32(e, h, cv::arg::<E>(args, 0))?;
        impls::html_collection::item(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn html_collection_item_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, html_collection_item::<E>) } }

fn html_collection_named_item<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    cv::run(e, h, |e, h| {
        let this = cv::this_side(e, h, this, &HTMLCollectionClass::<E>::DEF)?;
        cv::need(e, args, 1)?;
        let a0 = cv::dom_string(e, h, cv::arg::<E>(args, 0))?;
        impls::html_collection::named_item(e, h, this, a0)
    })
}

// SAFETY: called as the host function of its table entry
unsafe extern "C" fn html_collection_named_item_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 { unsafe { shell::op(ctx, this, argv, argc, html_collection_named_item::<E>) } }
