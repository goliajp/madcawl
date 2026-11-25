//! The four interfaces on the stub engine, driven as script drives
//! them.

mod support;

use madcawl_bindings::{DomHost, NodeClass};
use madcawl_dom::Dom;
use madcawl_html::{Options, Step, Tokenizer, TreeBuilder};
use madcawl_js::{Engine, Key};
use madcawl_js_stub::{Stub, Value};
use madcawl_str::{DomString, atom};
use support::{Js, V};

fn parsed(html: &str) -> Js {
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(Dom::new(), Options::default());
    tok.feed(html.as_bytes());
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    Js::with(b.into_sink())
}

#[test]
fn elements_are_made_and_moved_and_read_back_as_the_same_wrappers() {
    let mut js = Js::new();
    let doc = js.doc();
    let name = js.str("HTML");
    let html = js.call(doc.get(), "createElement", &[name.get()]);
    js.drop(name);
    assert_eq!(js.read(html.get(), "tagName"), "HTML");
    assert_eq!(js.read(html.get(), "localName"), "html");
    assert_eq!(js.read(html.get(), "nodeType"), "1");
    assert_eq!(js.read(html.get(), "isConnected"), "false");
    assert_eq!(js.read(doc.get(), "documentElement"), "null");
    js.run(doc.get(), "appendChild", &[html.get()]);
    assert!(js.is(doc.get(), "documentElement", html.get()));
    let div_name = js.str("div");
    let div = js.call(doc.get(), "createElement", &[div_name.get()]);
    js.drop(div_name);
    let r = js.call(html.get(), "appendChild", &[div.get()]);
    assert_eq!(r.get(), div.get(), "appendChild returns its argument");
    js.drop(r);
    assert!(js.is(div.get(), "parentNode", html.get()));
    assert!(js.is(div.get(), "parentElement", html.get()));
    assert!(js.is(html.get(), "firstChild", div.get()));
    assert!(js.is(div.get(), "ownerDocument", doc.get()));
    assert_eq!(js.read(doc.get(), "ownerDocument"), "null");
    assert_eq!(js.read(div.get(), "isConnected"), "true");
    assert_eq!(js.read(doc.get(), "nodeName"), "#document");
    let gone = js.call(html.get(), "removeChild", &[div.get()]);
    js.drop(gone);
    assert_eq!(js.read(div.get(), "parentNode"), "null");
    let has = js.call(html.get(), "hasChildNodes", &[]);
    assert_eq!(has.get(), Stub::boolean(false));
    js.audit();
    for v in [div, html, doc] {
        js.drop(v);
    }
}

#[test]
fn text_content_reads_and_replaces_children() {
    let mut js = Js::new();
    let doc = js.doc();
    let name = js.str("p");
    let p = js.call(doc.get(), "createElement", &[name.get()]);
    let hello = js.str("hello");
    js.set(p.get(), "textContent", hello.get());
    let first = js.get(p.get(), "firstChild");
    assert_eq!(js.read(first.get(), "nodeType"), "3");
    assert_eq!(js.read(first.get(), "nodeName"), "#text");
    assert_eq!(js.read(p.get(), "textContent"), "hello");
    let data = js.call(doc.get(), "createTextNode", &[name.get()]);
    js.run(p.get(), "appendChild", &[data.get()]);
    assert_eq!(js.read(p.get(), "textContent"), "hellop");
    js.set(first.get(), "textContent", name.get());
    assert_eq!(js.read(p.get(), "textContent"), "pp");
    js.set(p.get(), "textContent", Stub::null());
    assert_eq!(js.read(p.get(), "firstChild"), "null");
    assert_eq!(js.read(first.get(), "parentNode"), "null");
    js.audit();
    for v in [data, first, hello, name, p, doc] {
        js.drop(v);
    }
}

#[test]
fn wrong_receivers_and_wrong_trees_throw_the_right_errors() {
    let mut js = Js::new();
    let doc = js.doc();
    let name = js.str("div");
    let div = js.call(doc.get(), "createElement", &[name.get()]);
    let span = js.call(doc.get(), "createElement", &[name.get()]);
    let text = js.call(doc.get(), "createTextNode", &[name.get()]);
    // an Element method on a text node
    let get_attribute = js.get(div.get(), "getAttribute");
    let r = js.e.call(&mut js.h, get_attribute.get(), text.get(), &[name.get()]);
    assert!(Stub::is_thrown(r.into_raw()));
    assert_eq!(js.exception(), "TypeError");
    js.drop(get_attribute);
    let plain = js.object();
    for (target, arg, want) in [
        (text.get(), div.get(), "HierarchyRequestError"),
        (div.get(), plain.get(), "TypeError"),
        (div.get(), doc.get(), "HierarchyRequestError"),
    ] {
        let r = js.call(target, "appendChild", &[arg]);
        assert!(Stub::is_thrown(r.into_raw()));
        assert_eq!(js.exception(), want);
    }
    js.run(div.get(), "appendChild", &[span.get()]);
    let r = js.call(span.get(), "appendChild", &[div.get()]);
    assert!(Stub::is_thrown(r.into_raw()));
    assert_eq!(js.exception(), "HierarchyRequestError", "an ancestor into its descendant");
    let r = js.call(span.get(), "removeChild", &[div.get()]);
    assert!(Stub::is_thrown(r.into_raw()));
    assert_eq!(js.exception(), "NotFoundError");
    let r = js.call(div.get(), "appendChild", &[]);
    assert!(Stub::is_thrown(r.into_raw()));
    assert_eq!(js.exception(), "TypeError");
    let bad = js.str("a b");
    let r = js.call(doc.get(), "createElement", &[bad.get()]);
    assert!(Stub::is_thrown(r.into_raw()));
    assert_eq!(js.exception(), "InvalidCharacterError");
    js.audit();
    for v in [bad, plain, text, span, div, name, doc] {
        js.drop(v);
    }
}

#[test]
fn methods_constants_and_readonly_attributes_behave_as_members() {
    let mut js = Js::new();
    let doc = js.doc();
    let name = js.str("div");
    let div = js.call(doc.get(), "createElement", &[name.get()]);
    let f = js.get(div.get(), "appendChild");
    assert!(js.is(doc.get(), "appendChild", f.get()), "one function per realm");
    assert_eq!(js.read(f.get(), "length"), "1");
    assert_eq!(js.read(f.get(), "name"), "appendChild");
    assert_eq!(js.read(div.get(), "ELEMENT_NODE"), "1");
    assert_eq!(js.read(doc.get(), "DOCUMENT_FRAGMENT_NODE"), "11");
    js.set(div.get(), "nodeType", name.get());
    assert_eq!(js.read(div.get(), "nodeType"), "1", "a readonly attribute ignores writes");
    assert_eq!(js.h.dom.wrapper(js.node(div.get())) & 1, 0, "and makes no own property");
    for v in [f, div, name, doc] {
        js.drop(v);
    }
}

#[test]
fn attributes_and_ids_of_a_parsed_document() {
    let mut js = parsed("<p id=intro class='a b' data-x=1>hi</p><svg viewBox='0 0 1 1'></svg>");
    let doc = js.doc();
    let id = js.str("intro");
    let p = js.call(doc.get(), "getElementById", &[id.get()]);
    assert_eq!(js.read(p.get(), "id"), "intro");
    assert_eq!(js.read(p.get(), "className"), "a b");
    let key = js.str("DATA-X");
    let v = js.call(p.get(), "getAttribute", &[key.get()]);
    assert_eq!(js.text(v.get()), "1");
    let missing = js.str("nope");
    let none = js.call(p.get(), "getAttribute", &[missing.get()]);
    assert_eq!(none.get(), Stub::null());
    let has = js.call(p.get(), "hasAttribute", &[key.get()]);
    assert_eq!(has.get(), Stub::boolean(true));
    let absent = js.call(doc.get(), "getElementById", &[missing.get()]);
    assert_eq!(absent.get(), Stub::null());
    let html = js.get(doc.get(), "documentElement");
    let body = js.get(html.get(), "lastChild");
    let svg = js.get(body.get(), "lastChild");
    assert_eq!(js.read(svg.get(), "tagName"), "svg", "not upper-cased outside HTML");
    for v in [svg, body, html, absent, has, none, missing, v, key, p, id, doc] {
        js.drop(v);
    }
    js.audit();
}

#[test]
fn listeners_are_added_once_called_and_removed() {
    let mut js = Js::new();
    let doc = js.doc();
    let name = js.str("div");
    let div = js.call(doc.get(), "createElement", &[name.get()]);
    let log = js.object();
    let log2 = js.retain(log.get());
    let f = js.closure(vec![log2]);
    let click = js.str("click");
    js.run(div.get(), "addEventListener", &[click.get(), f.get()]);
    js.run(div.get(), "addEventListener", &[click.get(), f.get()]);
    js.run(div.get(), "addEventListener", &[click.get(), f.get(), Stub::boolean(true)]);
    js.run(div.get(), "addEventListener", &[click.get(), Stub::null()]);
    let n = js.node(div.get());
    assert_eq!(js.h.listener_count(n), 2, "the same listener once per capture flag");
    assert_eq!(js.h.fire(&mut js.e, n, atom!("click")), 2);
    assert_eq!(js.read(log.get(), "calls"), "2");
    js.run(div.get(), "removeEventListener", &[click.get(), f.get(), Stub::boolean(true)]);
    assert_eq!(js.h.listener_count(n), 1);
    js.set(log.get(), "fail", Stub::boolean(true));
    assert_eq!(js.h.fire(&mut js.e, n, atom!("click")), 1);
    assert_eq!(js.h.reported, 1, "an exception no one catches goes to the host");
    js.run(div.get(), "removeEventListener", &[click.get(), f.get()]);
    assert_eq!(js.h.listener_count(n), 0);
    let before = js.e.count_of(f.get());
    assert_eq!(before, 1, "only the test holds the listener now");
    for v in [click, f, log, div, name, doc] {
        js.drop(v);
    }
}

#[test]
fn an_event_type_is_one_name_however_it_is_spelled_as_a_value() {
    let mut js = Js::new();
    let doc = js.doc();
    let name = js.str("div");
    let div = js.call(doc.get(), "createElement", &[name.get()]);
    let log = js.object();
    let f = js.closure(vec![log]);
    let n = js.node(div.get());
    // a name no static atom spells, as two separate strings
    let (made, again) = (js.str("made-up-type"), js.str("made-up-type"));
    js.run(div.get(), "addEventListener", &[made.get(), f.get()]);
    js.run(div.get(), "addEventListener", &[again.get(), f.get()]);
    assert_eq!(js.h.listener_count(n), 1, "both strings name one atom");
    // a number is converted to its string first
    js.run(div.get(), "addEventListener", &[Stub::number(7.0), f.get()]);
    assert_eq!(js.h.listener_count(n), 2);
    let seven = js.str("7");
    js.run(div.get(), "removeEventListener", &[seven.get(), f.get()]);
    js.run(div.get(), "removeEventListener", &[again.get(), f.get()]);
    assert_eq!(js.h.listener_count(n), 0);
    for v in [seven, again, made, f, div, name, doc] {
        js.drop(v);
    }
}

#[test]
fn a_host_closed_gives_back_what_it_held() {
    let mut js = Js::new();
    let doc = js.doc();
    let name = js.str("div");
    let div = js.call(doc.get(), "createElement", &[name.get()]);
    let html = js.call(doc.get(), "createElement", &[name.get()]);
    js.run(doc.get(), "appendChild", &[html.get()]);
    js.run(html.get(), "appendChild", &[div.get()]);
    js.set(div.get(), "x", name.get());
    let f = js.closure(vec![]);
    let click = js.str("click");
    js.run(div.get(), "addEventListener", &[click.get(), f.get()]);
    for v in [click, f, html, div, name, doc] {
        js.drop(v);
    }
    let Js { mut e, h } = js;
    assert!(e.live_cells() > 1);
    h.close(&mut e);
    assert_eq!(e.live_cells(), 0, "no cell outlives its host");
}

#[test]
fn strings_that_are_views_reach_the_dom_as_flat_blocks() {
    let mut js = Js::new();
    let doc = js.doc();
    let whole = js.str("xdivhello");
    let name = js.e.substring(whole.get(), 1, 3);
    let el = js.call(doc.get(), "createElement", &[name.get()]);
    assert_eq!(js.read(el.get(), "tagName"), "DIV");
    let text = js.e.substring(whole.get(), 4, 5);
    js.set(el.get(), "textContent", text.get());
    assert_eq!(js.read(el.get(), "textContent"), "hello");
    js.audit();
    for v in [text, el, name, whole, doc] {
        js.drop(v);
    }
}

fn key(name: &str) -> DomString {
    DomString::from_wtf8(name.as_bytes())
}

fn number(e: &mut Stub, h: &mut DomHost<Stub>, obj: Value, name: &str) -> f64 {
    let v = e.get(h, obj, Key::Str(&key(name)));
    let n = e.as_number(v.get()).unwrap_or(0.0);
    e.release(v);
    n
}

/// A listener that adds the log's `mt` to its `seen`.
fn note(e: &mut Stub, h: &mut DomHost<Stub>, caps: &[Value], _: Value, _: &[Value]) -> V {
    let seen = number(e, h, caps[0], "seen") + number(e, h, caps[0], "mt");
    assert!(e.set(h, caps[0], Key::Str(&key("seen")), Stub::number(seen)));
    support::undefined()
}

/// A microtask that sets the log's `mt`.
fn mark(e: &mut Stub, h: &mut DomHost<Stub>, caps: &[Value], _: Value, _: &[Value]) -> V {
    assert!(e.set(h, caps[0], Key::Str(&key("mt")), Stub::number(1.0)));
    support::undefined()
}

/// A listener that queues a microtask and dispatches to another node,
/// as `other.click()` in script does.
fn dispatch(e: &mut Stub, h: &mut DomHost<Stub>, caps: &[Value], _: Value, _: &[Value]) -> V {
    let job = e.retain(caps[2]);
    e.enqueue_microtask(job, support::undefined());
    let other = e.host_of(caps[1], &NodeClass::<Stub>::DEF).expect("a node");
    let other = h.node(other);
    assert_eq!(h.fire(e, other, atom!("click")), 2);
    support::undefined()
}

#[test]
fn a_dispatch_script_started_runs_no_checkpoint_between_its_listeners() {
    let mut js = Js::new();
    let doc = js.doc();
    let name = js.str("div");
    let a = js.call(doc.get(), "createElement", &[name.get()]);
    let b = js.call(doc.get(), "createElement", &[name.get()]);
    let log = js.object();
    let click = js.str("click");
    for _ in 0..2 {
        let l = js.retain(log.get());
        let f = js.e.new_closure::<DomHost<Stub>>(js.h.realm, note, vec![l]);
        js.run(b.get(), "addEventListener", &[click.get(), f.get()]);
        js.drop(f);
    }
    let l = js.retain(log.get());
    let job = js.e.new_closure::<DomHost<Stub>>(js.h.realm, mark, vec![l]);
    let caps = vec![js.retain(log.get()), js.retain(b.get()), job];
    let f = js.e.new_closure::<DomHost<Stub>>(js.h.realm, dispatch, caps);
    js.run(a.get(), "addEventListener", &[click.get(), f.get()]);
    let n = js.node(a.get());
    assert_eq!(js.h.fire(&mut js.e, n, atom!("click")), 1);
    assert_eq!(js.read(log.get(), "seen"), "0", "the microtask waited for the outer listener");
    assert_eq!(js.read(log.get(), "mt"), "1", "and ran at the checkpoint after it");
    assert!(!js.e.in_script());
    for v in [f, click, log, b, a, name, doc] {
        js.drop(v);
    }
}

#[test]
fn a_dom_exception_is_an_object_the_host_made_with_name_message_and_code() {
    let mut js = Js::new();
    let doc = js.doc();
    let bad = js.str("a b");
    let r = js.call(doc.get(), "createElement", &[bad.get()]);
    assert!(Stub::is_thrown(r.into_raw()));
    let ex = js.e.take_exception().expect("a DOMException");
    assert_eq!(js.read(ex.get(), "name"), "InvalidCharacterError");
    assert_eq!(js.read(ex.get(), "code"), "5");
    assert_eq!(js.read(ex.get(), "message"), "not a valid element name");
    for v in [ex, bad, doc] {
        js.drop(v);
    }
}
