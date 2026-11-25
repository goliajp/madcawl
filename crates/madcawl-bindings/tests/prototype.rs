//! The prototype chain of the bound interfaces: interface objects and
//! `instanceof`, one function object per method however it is reached,
//! and a getter taken off a prototype and called on another object.

mod support;

use madcawl_bindings::{DocumentClass, DomHost, ElementClass, EventTargetClass, NodeClass};
use madcawl_js::Engine;
use madcawl_js_stub::{Stub, Value};
use support::{Js, V};

type H = DomHost<Stub>;

struct Ifaces {
    event_target: V,
    node: V,
    element: V,
    document: V,
}

fn ifaces(js: &mut Js) -> Ifaces {
    let r = js.h.realm;
    Ifaces {
        event_target: js.e.interface_object(r, &EventTargetClass::<Stub>::DEF),
        node: js.e.interface_object(r, &NodeClass::<Stub>::DEF),
        element: js.e.interface_object(r, &ElementClass::<Stub>::DEF),
        document: js.e.interface_object(r, &DocumentClass::<Stub>::DEF),
    }
}

fn is_a(js: &mut Js, v: Value, ctor: &V) -> bool {
    js.e.instance_of(&mut js.h, v, ctor.get())
}

#[test]
fn interface_objects_answer_instanceof_along_the_class_chain() {
    let mut js = Js::new();
    let i = ifaces(&mut js);
    let again = js.e.interface_object(js.h.realm, &NodeClass::<Stub>::DEF);
    assert_eq!(again.get(), i.node.get(), "one interface object per realm");
    assert_eq!(js.read(i.node.get(), "name"), "Node");
    let div = js.element("div");
    let doc = js.doc();
    let data = js.str("t");
    let text = js.call(doc.get(), "createTextNode", &[data.get()]);
    for (v, ctor, want) in [
        (div.get(), &i.element, true),
        (div.get(), &i.node, true),
        (div.get(), &i.event_target, true),
        (div.get(), &i.document, false),
        (text.get(), &i.node, true),
        (text.get(), &i.element, false),
        (doc.get(), &i.document, true),
    ] {
        assert_eq!(is_a(&mut js, v, ctor), want);
    }
    // the prototype objects chain as the classes do
    let el_proto = js.get(i.element.get(), "prototype");
    let node_proto = js.get(i.node.get(), "prototype");
    let up = js.e.prototype_of::<H>(el_proto.get());
    assert_eq!(up.get(), node_proto.get());
    let div_proto = js.e.prototype_of::<H>(div.get());
    assert_eq!(div_proto.get(), el_proto.get());
    assert!(js.is(node_proto.get(), "constructor", i.node.get()));
    // an interface without a constructor throws when called
    let r = js.e.call(&mut js.h, i.node.get(), Stub::undefined(), &[]);
    assert!(Stub::is_thrown(r.into_raw()));
    assert_eq!(js.exception(), "TypeError");
    for v in [div_proto, up, node_proto, el_proto, text, data, doc, div, again] {
        js.drop(v);
    }
    for v in [i.document, i.element, i.node, i.event_target] {
        js.drop(v);
    }
}

#[test]
fn a_method_is_one_function_however_it_is_reached() {
    let mut js = Js::new();
    let i = ifaces(&mut js);
    let div = js.element("div");
    let doc = js.doc();
    let node_proto = js.get(i.node.get(), "prototype");
    let el_proto = js.get(i.element.get(), "prototype");
    let f = js.get(node_proto.get(), "appendChild");
    for v in [div.get(), doc.get(), el_proto.get()] {
        assert!(js.is(v, "appendChild", f.get()));
    }
    // a property script puts on a prototype is seen through wrappers
    js.set(node_proto.get(), "shared", Stub::number(5.0));
    assert_eq!(js.read(div.get(), "shared"), "5");
    // a wrapper given another prototype loses the inherited members
    let other = js.object();
    assert!(js.e.set_prototype(&mut js.h, div.get(), other.get()));
    let gone = js.get(div.get(), "appendChild");
    assert_eq!(gone.get(), Stub::undefined());
    assert!(!is_a(&mut js, div.get(), &i.node));
    for v in [gone, other, f, el_proto, node_proto, doc, div] {
        js.drop(v);
    }
    for v in [i.document, i.element, i.node, i.event_target] {
        js.drop(v);
    }
}

#[test]
fn a_getter_taken_off_a_prototype_checks_the_object_it_is_called_on() {
    let mut js = Js::new();
    let i = ifaces(&mut js);
    let div = js.element("div");
    let doc = js.doc();
    let data = js.str("t");
    let text = js.call(doc.get(), "createTextNode", &[data.get()]);
    let node_proto = js.get(i.node.get(), "prototype");
    let el_proto = js.get(i.element.get(), "prototype");
    let node_type = js.e.own_getter::<H>(node_proto.get(), "nodeType");
    let same = js.e.own_getter::<H>(node_proto.get(), "nodeType");
    assert_eq!(node_type.get(), same.get(), "one getter function per realm");
    for (this, want) in [(div.get(), 1.0), (text.get(), 3.0), (doc.get(), 9.0)] {
        let r = js.e.call(&mut js.h, node_type.get(), this, &[]);
        assert_eq!(js.e.as_number(r.get()), Some(want));
    }
    let plain = js.object();
    let tag_name = js.e.own_getter::<H>(el_proto.get(), "tagName");
    for (g, this) in [(&node_type, plain.get()), (&tag_name, text.get()), (&node_type, node_proto.get())] {
        let r = js.e.call(&mut js.h, g.get(), this, &[]);
        assert!(Stub::is_thrown(r.into_raw()));
        assert_eq!(js.exception(), "TypeError", "illegal invocation");
    }
    // reading the attribute on the prototype itself is such a call
    let r = js.e.get(&mut js.h, node_proto.get(), madcawl_js::Key::Str(&support::key("nodeType")));
    assert!(Stub::is_thrown(r.into_raw()));
    assert_eq!(js.exception(), "TypeError");
    let inherited = js.e.own_getter::<H>(el_proto.get(), "nodeType");
    assert_eq!(inherited.get(), Stub::undefined(), "Element.prototype does not own nodeType");
    for v in [inherited, tag_name, plain, same, node_type, el_proto, node_proto, text, data, doc, div] {
        js.drop(v);
    }
    for v in [i.document, i.element, i.node, i.event_target] {
        js.drop(v);
    }
    let Js { mut e, h } = js;
    h.close(&mut e);
    assert_eq!(e.live_cells(), 0, "the realm gave up its interface and prototype objects");
}
