//! Argument conversions that run script: an object's `toString` changes
//! the tree, removes listeners or throws, in every argument position
//! that converts by ToString; a Symbol cannot be converted. Conversions
//! run in argument order, after the receiver's brand check and before
//! the member acts.

mod support;

use madcawl_bindings::DomHost;
use madcawl_js::Engine;
use madcawl_js_stub::{Stub, Value};
use madcawl_str::atom;
use support::{Js, V, call_in};

/// `toString` that returns its first capture.
fn says(e: &mut Stub, _: &mut DomHost<Stub>, caps: &[Value], _: Value, _: &[Value]) -> V {
    e.retain(caps[0])
}

/// `toString` that throws its first capture.
fn throws(e: &mut Stub, _: &mut DomHost<Stub>, caps: &[Value], _: Value, _: &[Value]) -> V {
    let ex = e.retain(caps[0]);
    e.throw(ex)
}

/// `toString` that takes node `caps[1]` out of its parent, then
/// returns `caps[0]`.
fn detaches(e: &mut Stub, h: &mut DomHost<Stub>, caps: &[Value], _: Value, _: &[Value]) -> V {
    let parent = e.get(h, caps[1], madcawl_js::Key::Str(&support::key("parentNode")));
    if parent.get() != Stub::null() {
        assert!(!call_in(e, h, parent.get(), "removeChild", &[caps[1]]));
    }
    e.release(parent);
    e.retain(caps[0])
}

/// `toString` that removes listener `caps[3]` of type `caps[2]` from
/// `caps[1]`, then returns `caps[0]`.
fn unlistens(e: &mut Stub, h: &mut DomHost<Stub>, caps: &[Value], _: Value, _: &[Value]) -> V {
    assert!(!call_in(e, h, caps[1], "removeEventListener", &[caps[2], caps[3]]));
    e.retain(caps[0])
}

/// An object whose `toString` is `body` over `captures`.
fn stringer(js: &mut Js, body: madcawl_js_stub::ClosureFn<DomHost<Stub>>, captures: Vec<V>) -> V {
    let f = js.function(body, captures);
    let o = js.object();
    js.set(o.get(), "toString", f.get());
    js.drop(f);
    o
}

#[test]
fn an_event_type_object_converts_through_its_tostring() {
    let mut js = Js::new();
    let div = js.element("div");
    let log = js.object();
    let l = js.retain(log.get());
    let f = js.closure(vec![l]);
    let click = js.str("click");
    let ty = stringer(&mut js, says, vec![click]);
    js.run(div.get(), "addEventListener", &[ty.get(), f.get()]);
    let n = js.node(div.get());
    assert_eq!(js.h.listener_count(n), 1);
    assert_eq!(js.h.fire(&mut js.e, n, atom!("click")), 1, "added under the name toString gave");
    js.run(div.get(), "removeEventListener", &[ty.get(), f.get()]);
    assert_eq!(js.h.listener_count(n), 0);
    for v in [ty, f, log, div] {
        js.drop(v);
    }
}

#[test]
fn an_event_type_whose_tostring_throws_adds_nothing_and_its_exception_wins() {
    let mut js = Js::new();
    let div = js.element("div");
    let f = js.closure(vec![]);
    let mine = js.object();
    let m = js.retain(mine.get());
    let ty = stringer(&mut js, throws, vec![m]);
    // the callback argument is wrong too, but the type is converted first
    for cb in [f.get(), Stub::number(1.0)] {
        let r = js.call(div.get(), "addEventListener", &[ty.get(), cb]);
        assert!(Stub::is_thrown(r.into_raw()));
        let ex = js.e.take_exception().expect("the exception toString threw");
        assert_eq!(ex.get(), mine.get());
        js.drop(ex);
    }
    assert_eq!(js.h.listener_count(js.node(div.get())), 0);
    for v in [ty, mine, f, div] {
        js.drop(v);
    }
}

#[test]
fn a_symbol_event_type_throws_a_type_error_and_adds_nothing() {
    let mut js = Js::new();
    let div = js.element("div");
    let f = js.closure(vec![]);
    let sym = js.e.new_symbol(js.h.realm);
    for op in ["addEventListener", "removeEventListener"] {
        let r = js.call(div.get(), op, &[sym.get(), f.get()]);
        assert!(Stub::is_thrown(r.into_raw()));
        assert_eq!(js.exception(), "TypeError");
    }
    assert_eq!(js.h.listener_count(js.node(div.get())), 0);
    for v in [sym, f, div] {
        js.drop(v);
    }
}

#[test]
fn a_tostring_that_removes_a_listener_runs_before_the_add() {
    let mut js = Js::new();
    let div = js.element("div");
    let (log1, log2) = (js.object(), js.object());
    let l1 = js.retain(log1.get());
    let f1 = js.closure(vec![l1]);
    let l2 = js.retain(log2.get());
    let f2 = js.closure(vec![l2]);
    let click = js.str("click");
    js.run(div.get(), "addEventListener", &[click.get(), f1.get()]);
    let caps = vec![js.retain(click.get()), js.retain(div.get()), js.retain(click.get()), js.retain(f1.get())];
    let ty = stringer(&mut js, unlistens, caps);
    js.run(div.get(), "addEventListener", &[ty.get(), f2.get()]);
    let n = js.node(div.get());
    assert_eq!(js.h.listener_count(n), 1);
    assert_eq!(js.h.fire(&mut js.e, n, atom!("click")), 1);
    assert_eq!((js.number(log1.get(), "calls"), js.number(log2.get(), "calls")), (0.0, 1.0));
    for v in [ty, click, f2, f1, log2, log1, div] {
        js.drop(v);
    }
}

/// A call of one string-taking member on `recv` with `arg` as its
/// string argument; whether it threw.
type Member = fn(&mut Js, Value, Value) -> bool;

fn call_member(js: &mut Js, recv: Value, name: &str, args: &[Value]) -> bool {
    let r = js.call(recv, name, args);
    let threw = Stub::is_thrown(r.get());
    if threw {
        let _sentinel = r.into_raw();
    } else {
        js.drop(r);
    }
    threw
}

/// Every argument position bound so far that converts by ToString, on
/// an element `el` (in a document) and the document `doc`.
fn positions() -> Vec<(&'static str, Member)> {
    fn listener(js: &mut Js, el: Value, arg: Value, op: &str) -> bool {
        let f = js.closure(vec![]);
        let threw = call_member(js, el, op, &[arg, f.get()]);
        js.drop(f);
        threw
    }
    vec![
        ("addEventListener", |js, el, a| listener(js, el, a, "addEventListener")),
        ("removeEventListener", |js, el, a| listener(js, el, a, "removeEventListener")),
        ("getAttribute", |js, el, a| call_member(js, el, "getAttribute", &[a])),
        ("hasAttribute", |js, el, a| call_member(js, el, "hasAttribute", &[a])),
        ("textContent", |js, el, a| {
            let key = support::key("textContent");
            !js.e.set(&mut js.h, el, madcawl_js::Key::Str(&key), a)
        }),
        ("createElement", |js, _, a| {
            let doc = js.doc();
            let threw = call_member(js, doc.get(), "createElement", &[a]);
            js.drop(doc);
            threw
        }),
        ("createTextNode", |js, _, a| {
            let doc = js.doc();
            let threw = call_member(js, doc.get(), "createTextNode", &[a]);
            js.drop(doc);
            threw
        }),
        ("createComment", |js, _, a| {
            let doc = js.doc();
            let threw = call_member(js, doc.get(), "createComment", &[a]);
            js.drop(doc);
            threw
        }),
        ("getElementById", |js, _, a| {
            let doc = js.doc();
            let threw = call_member(js, doc.get(), "getElementById", &[a]);
            js.drop(doc);
            threw
        }),
    ]
}

#[test]
fn a_tostring_that_changes_the_tree_runs_first_in_every_position() {
    for (name, member) in positions() {
        let mut js = Js::new();
        let html = js.connected_root();
        let (el, victim) = (js.element("p"), js.element("i"));
        js.run(html.get(), "appendChild", &[el.get()]);
        js.run(html.get(), "appendChild", &[victim.get()]);
        let text = js.str("x");
        let v = js.retain(victim.get());
        let arg = stringer(&mut js, detaches, vec![text, v]);
        assert!(!member(&mut js, el.get(), arg.get()), "{name} threw");
        assert_eq!(js.read(victim.get(), "parentNode"), "null", "{name}: toString ran");
        assert_eq!(js.read(el.get(), "isConnected"), "true", "{name}: the member still acted on its receiver");
        js.audit();
        for v in [arg, victim, el, html] {
            js.drop(v);
        }
    }
}

#[test]
fn a_tostring_that_throws_stops_the_member_in_every_position() {
    for (name, member) in positions() {
        let mut js = Js::new();
        let html = js.connected_root();
        let el = js.element("p");
        js.run(html.get(), "appendChild", &[el.get()]);
        let before = js.h.dom.node_count();
        let mine = js.object();
        let m = js.retain(mine.get());
        let arg = stringer(&mut js, throws, vec![m]);
        assert!(member(&mut js, el.get(), arg.get()), "{name} did not throw");
        let ex = js.e.take_exception().expect("an exception");
        assert_eq!(ex.get(), mine.get(), "{name}: toString's own exception");
        js.drop(ex);
        assert_eq!(js.h.dom.node_count(), before, "{name}: nothing made, nothing removed");
        assert_eq!(js.h.listener_count(js.node(el.get())), 0, "{name}: no listener");
        js.audit();
        for v in [arg, mine, el, html] {
            js.drop(v);
        }
    }
}

#[test]
fn the_receiver_is_checked_before_any_argument_is_converted() {
    let mut js = Js::new();
    let div = js.element("div");
    let log = js.object();
    // a toString that counts its calls on log
    let ty = {
        let l = js.retain(log.get());
        let f = js.closure(vec![l]);
        let o = js.object();
        js.set(o.get(), "toString", f.get());
        js.drop(f);
        o
    };
    let add = js.get(div.get(), "addEventListener");
    let plain = js.object();
    let r = js.e.call(&mut js.h, add.get(), plain.get(), &[ty.get(), add.get()]);
    assert!(Stub::is_thrown(r.into_raw()));
    assert_eq!(js.exception(), "TypeError");
    assert_eq!(js.number(log.get(), "calls"), 0.0, "toString never ran");
    for v in [plain, add, ty, log, div] {
        js.drop(v);
    }
}
