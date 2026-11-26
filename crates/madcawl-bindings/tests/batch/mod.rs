//! The binding tests that run on every engine: written against the
//! engine interface only, driven as a page's script would drive the
//! bindings. Each engine's test file implements [`Driver`] and has one
//! `#[test]` per function here; the results must be the same.

use madcawl_bindings::{DomHost, NodeClass};
use madcawl_dom::{Dom, NodeId};
use madcawl_html::{Options, Step, Tokenizer, TreeBuilder};
use madcawl_js::{Engine, Key, Owned};
use madcawl_str::{DomString, atom};

/// What a test needs of an engine beyond the interface: a fresh
/// isolate, and a script function, which only script can make.
pub trait Driver: Engine {
    fn make() -> Self;
    /// A function that adds one to `log.calls` each time it is called,
    /// and throws a TypeError when `log.fail` is truthy.
    fn counting(&mut self, h: &mut DomHost<Self>, log: Self::Value) -> Owned<Self::Value>;
}

pub struct Js<E: Driver> {
    pub e: E,
    pub h: DomHost<E>,
}

impl<E: Driver> Js<E> {
    pub fn new() -> Self {
        Self::with(Dom::new())
    }

    pub fn with(dom: Dom) -> Self {
        let mut e = E::make();
        let h = DomHost::new(&mut e, dom);
        Js { e, h }
    }

    pub fn parsed(html: &str) -> Self {
        let mut tok = Tokenizer::new();
        let mut b = TreeBuilder::new(Dom::new(), Options::default());
        tok.feed(html.as_bytes());
        tok.end();
        assert_eq!(tok.run(&mut b), Step::Done);
        Self::with(b.into_sink())
    }

    pub fn doc(&mut self) -> Owned<E::Value> {
        let d = self.h.dom.document();
        self.h.wrap(&mut self.e, d)
    }

    pub fn node(&self, v: E::Value) -> NodeId {
        let obj = self.e.host_of(v, &NodeClass::<E>::DEF).expect("a node");
        self.h.node(obj)
    }

    pub fn str(&mut self, s: &str) -> Owned<E::Value> {
        self.e.string(DomString::from_wtf8(s.as_bytes()))
    }

    pub fn get(&mut self, obj: E::Value, name: &str) -> Owned<E::Value> {
        let r = self.e.get(&mut self.h, obj, Key::Str(&DomString::from_wtf8(name.as_bytes())));
        assert!(!E::is_thrown(r.get()), "reading {name} threw");
        r
    }

    pub fn set(&mut self, obj: E::Value, name: &str, v: E::Value) {
        assert!(self.e.set(&mut self.h, obj, Key::Str(&DomString::from_wtf8(name.as_bytes())), v));
    }

    pub fn call(&mut self, obj: E::Value, name: &str, args: &[E::Value]) -> Owned<E::Value> {
        let f = self.get(obj, name);
        let r = self.e.call(&mut self.h, f.get(), obj, args);
        self.e.release(f);
        r
    }

    pub fn run(&mut self, obj: E::Value, name: &str, args: &[E::Value]) {
        let r = self.call(obj, name, args);
        assert!(!E::is_thrown(r.get()), "{name} threw {}", self.exception());
        self.e.release(r);
    }

    /// The call threw; the exception's name.
    pub fn threw(&mut self, r: Owned<E::Value>) -> String {
        assert!(E::is_thrown(r.into_raw()), "the call threw");
        self.exception()
    }

    pub fn text(&mut self, v: E::Value) -> String {
        let s = self.e.as_string(v).expect("a string");
        let mut b = Vec::new();
        s.write_wtf8(&mut b);
        String::from_utf8(b).unwrap()
    }

    /// The text of `obj.name`, or "null".
    pub fn read(&mut self, obj: E::Value, name: &str) -> String {
        let v = self.get(obj, name);
        let out = match self.e.as_number(v.get()) {
            Some(n) => n.to_string(),
            None if v.get() == E::null() => "null".to_owned(),
            None if self.e.as_boolean(v.get()).is_some() => self.e.to_boolean(v.get()).to_string(),
            None => self.text(v.get()),
        };
        self.e.release(v);
        out
    }

    pub fn is(&mut self, obj: E::Value, name: &str, other: E::Value) -> bool {
        let v = self.get(obj, name);
        let same = v.get() == other;
        self.e.release(v);
        same
    }

    pub fn exception(&mut self) -> String {
        let Some(x) = self.e.take_exception() else { return "none".to_owned() };
        let name = self.read(x.get(), "name");
        self.e.release(x);
        name
    }

    pub fn drop(&mut self, v: Owned<E::Value>) {
        self.e.release(v);
        self.h.settle(&mut self.e);
    }

    pub fn drop_all<const N: usize>(&mut self, vs: [Owned<E::Value>; N]) {
        for v in vs {
            self.drop(v);
        }
    }

    pub fn audit(&self) {
        let a = self.h.dom.audit();
        assert_eq!(a.mismatches(), 0, "{a:?}");
    }

    pub fn element(&mut self, doc: E::Value, name: &str) -> Owned<E::Value> {
        let n = self.str(name);
        let el = self.call(doc, "createElement", &[n.get()]);
        assert!(!E::is_thrown(el.get()), "createElement threw");
        self.drop(n);
        el
    }
}

pub fn elements_are_made_moved_and_read_back_as_the_same_wrappers<E: Driver>() {
    let mut js = Js::<E>::new();
    let doc = js.doc();
    let html = js.element(doc.get(), "HTML");
    assert_eq!(js.read(html.get(), "tagName"), "HTML");
    assert_eq!(js.read(html.get(), "localName"), "html");
    assert_eq!(js.read(html.get(), "nodeType"), "1");
    assert_eq!(js.read(html.get(), "isConnected"), "false");
    assert_eq!(js.read(doc.get(), "documentElement"), "null");
    js.run(doc.get(), "appendChild", &[html.get()]);
    assert!(js.is(doc.get(), "documentElement", html.get()));
    let div = js.element(doc.get(), "div");
    let r = js.call(html.get(), "appendChild", &[div.get()]);
    assert!(r.get() == div.get(), "appendChild returns its argument");
    js.drop(r);
    assert!(js.is(div.get(), "parentNode", html.get()));
    assert!(js.is(html.get(), "firstChild", div.get()));
    assert!(js.is(div.get(), "ownerDocument", doc.get()));
    assert_eq!(js.read(doc.get(), "ownerDocument"), "null");
    assert_eq!(js.read(div.get(), "isConnected"), "true");
    assert_eq!(js.read(doc.get(), "nodeName"), "#document");
    let gone = js.call(html.get(), "removeChild", &[div.get()]);
    js.drop(gone);
    assert_eq!(js.read(div.get(), "parentNode"), "null");
    let has = js.call(html.get(), "hasChildNodes", &[]);
    assert!(has.get() == E::boolean(false));
    js.audit();
    js.drop_all([div, html, doc]);
}

pub fn text_content_reads_and_replaces_children<E: Driver>() {
    let mut js = Js::<E>::new();
    let doc = js.doc();
    let p = js.element(doc.get(), "p");
    let hello = js.str("hello");
    js.set(p.get(), "textContent", hello.get());
    let first = js.get(p.get(), "firstChild");
    assert_eq!(js.read(first.get(), "nodeName"), "#text");
    assert_eq!(js.read(p.get(), "textContent"), "hello");
    let data = js.call(doc.get(), "createTextNode", &[hello.get()]);
    js.run(p.get(), "appendChild", &[data.get()]);
    assert_eq!(js.read(p.get(), "textContent"), "hellohello");
    js.set(p.get(), "textContent", E::null());
    assert_eq!(js.read(p.get(), "firstChild"), "null");
    assert_eq!(js.read(first.get(), "parentNode"), "null");
    js.audit();
    js.drop_all([data, first, hello, p, doc]);
}

pub fn wrong_receivers_and_wrong_trees_throw_the_right_errors<E: Driver>() {
    let mut js = Js::<E>::new();
    let doc = js.doc();
    let div = js.element(doc.get(), "div");
    let span = js.element(doc.get(), "span");
    let name = js.str("x");
    let text = js.call(doc.get(), "createTextNode", &[name.get()]);
    let get_attribute = js.get(div.get(), "getAttribute");
    let r = js.e.call(&mut js.h, get_attribute.get(), text.get(), &[name.get()]);
    assert_eq!(js.threw(r), "TypeError");
    let plain = js.e.new_object(js.h.realm);
    for (target, arg, want) in [
        (text.get(), div.get(), "HierarchyRequestError"),
        (div.get(), plain.get(), "TypeError"),
        (div.get(), doc.get(), "HierarchyRequestError"),
    ] {
        let r = js.call(target, "appendChild", &[arg]);
        assert_eq!(js.threw(r), want);
    }
    js.run(div.get(), "appendChild", &[span.get()]);
    let r = js.call(span.get(), "appendChild", &[div.get()]);
    assert_eq!(js.threw(r), "HierarchyRequestError");
    let r = js.call(span.get(), "removeChild", &[div.get()]);
    assert_eq!(js.threw(r), "NotFoundError");
    let r = js.call(div.get(), "appendChild", &[]);
    assert_eq!(js.threw(r), "TypeError");
    js.audit();
    js.drop_all([get_attribute, plain, text, name, span, div, doc]);
}

pub fn methods_constants_and_readonly_attributes_behave_as_members<E: Driver>() {
    let mut js = Js::<E>::new();
    let doc = js.doc();
    let div = js.element(doc.get(), "div");
    let f = js.get(div.get(), "appendChild");
    assert!(js.is(doc.get(), "appendChild", f.get()), "one function per realm");
    assert_eq!(js.read(f.get(), "length"), "1");
    assert_eq!(js.read(f.get(), "name"), "appendChild");
    assert_eq!(js.read(div.get(), "ELEMENT_NODE"), "1");
    assert_eq!(js.read(doc.get(), "DOCUMENT_FRAGMENT_NODE"), "11");
    let s = js.str("x");
    js.set(div.get(), "nodeType", s.get());
    assert_eq!(js.read(div.get(), "nodeType"), "1", "a readonly attribute ignores writes");
    js.drop_all([s, f, div, doc]);
}

pub fn interface_objects_hold_the_prototype_chain<E: Driver>() {
    let mut js = Js::<E>::new();
    let realm = js.h.realm;
    let node = js.e.interface_object(realm, &NodeClass::<E>::DEF);
    let element = js.e.interface_object(realm, &madcawl_bindings::ElementClass::<E>::DEF);
    assert_eq!(js.read(node.get(), "ELEMENT_NODE"), "1");
    let doc = js.doc();
    let div = js.element(doc.get(), "div");
    let proto = js.get(element.get(), "prototype");
    let f = js.get(proto.get(), "getAttribute");
    let mine = js.get(div.get(), "getAttribute");
    assert!(f.get() == mine.get(), "a wrapper's members come from its class's prototype");
    let r = js.e.call(&mut js.h, node.get(), E::undefined(), &[]);
    assert_eq!(js.threw(r), "TypeError", "an interface without a constructor");
    js.drop_all([mine, f, proto, div, doc, element, node]);
}

pub fn attributes_and_ids_of_a_parsed_document<E: Driver>() {
    let mut js = Js::<E>::parsed("<p id=intro class='a b' data-x=1>hi</p><svg viewBox='0 0 1 1'></svg>");
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
    assert!(none.get() == E::null());
    let html = js.get(doc.get(), "documentElement");
    let body = js.get(html.get(), "lastChild");
    let svg = js.get(body.get(), "lastChild");
    assert_eq!(js.read(svg.get(), "tagName"), "svg", "not upper-cased outside HTML");
    js.drop_all([svg, body, html, none, missing, v, key, p, id, doc]);
    js.audit();
}

pub fn listeners_are_added_once_called_and_removed<E: Driver>() {
    let mut js = Js::<E>::new();
    let doc = js.doc();
    let div = js.element(doc.get(), "div");
    let log = js.e.new_object(js.h.realm);
    let f = js.e.counting(&mut js.h, log.get());
    let click = js.str("click");
    js.run(div.get(), "addEventListener", &[click.get(), f.get()]);
    js.run(div.get(), "addEventListener", &[click.get(), f.get()]);
    js.run(div.get(), "addEventListener", &[click.get(), f.get(), E::boolean(true)]);
    js.run(div.get(), "addEventListener", &[click.get(), E::null()]);
    let n = js.node(div.get());
    assert_eq!(js.h.listener_count(n), 2, "the same listener once per capture flag");
    assert_eq!(js.h.fire(&mut js.e, n, atom!("click")), 2);
    assert_eq!(js.read(log.get(), "calls"), "2");
    js.run(div.get(), "removeEventListener", &[click.get(), f.get(), E::boolean(true)]);
    assert_eq!(js.h.listener_count(n), 1);
    js.set(log.get(), "fail", E::boolean(true));
    assert_eq!(js.h.fire(&mut js.e, n, atom!("click")), 1);
    assert_eq!(js.h.reported, 1, "an exception no one catches goes to the host");
    js.run(div.get(), "removeEventListener", &[click.get(), f.get()]);
    assert_eq!(js.h.listener_count(n), 0);
    js.drop_all([click, f, log, div, doc]);
}

pub fn an_event_type_is_one_name_however_it_is_spelled<E: Driver>() {
    let mut js = Js::<E>::new();
    let doc = js.doc();
    let div = js.element(doc.get(), "div");
    let log = js.e.new_object(js.h.realm);
    let f = js.e.counting(&mut js.h, log.get());
    let n = js.node(div.get());
    let (made, again) = (js.str("made-up-type"), js.str("made-up-type"));
    js.run(div.get(), "addEventListener", &[made.get(), f.get()]);
    js.run(div.get(), "addEventListener", &[again.get(), f.get()]);
    assert_eq!(js.h.listener_count(n), 1, "both strings name one atom");
    js.run(div.get(), "addEventListener", &[E::number(7.0), f.get()]);
    assert_eq!(js.h.listener_count(n), 2, "a number is converted to its string");
    let seven = js.str("7");
    js.run(div.get(), "removeEventListener", &[seven.get(), f.get()]);
    js.run(div.get(), "removeEventListener", &[again.get(), f.get()]);
    assert_eq!(js.h.listener_count(n), 0);
    js.drop_all([seven, again, made, f, log, div, doc]);
}

pub fn a_dom_exception_carries_name_message_and_code<E: Driver>() {
    let mut js = Js::<E>::new();
    let doc = js.doc();
    let bad = js.str("a b");
    let r = js.call(doc.get(), "createElement", &[bad.get()]);
    assert!(E::is_thrown(r.into_raw()));
    let ex = js.e.take_exception().expect("a DOMException");
    assert_eq!(js.read(ex.get(), "name"), "InvalidCharacterError");
    assert_eq!(js.read(ex.get(), "code"), "5");
    assert_eq!(js.read(ex.get(), "message"), "not a valid element name");
    js.drop_all([ex, bad, doc]);
}

pub fn a_closed_host_leaves_a_sound_dom<E: Driver>() {
    let mut js = Js::<E>::new();
    let doc = js.doc();
    let html = js.element(doc.get(), "html");
    let div = js.element(doc.get(), "div");
    js.run(doc.get(), "appendChild", &[html.get()]);
    js.run(html.get(), "appendChild", &[div.get()]);
    let s = js.str("expando");
    js.set(div.get(), "x", s.get());
    let log = js.e.new_object(js.h.realm);
    let f = js.e.counting(&mut js.h, log.get());
    let click = js.str("click");
    js.run(div.get(), "addEventListener", &[click.get(), f.get()]);
    assert_eq!(js.read(div.get(), "x"), "expando");
    js.drop_all([click, f, log, s, div, html, doc]);
    js.audit();
    let Js { mut e, h } = js;
    h.close(&mut e);
}

pub fn timers_and_microtasks_run_on_the_event_loop<E: Driver>() {
    let mut js = Js::<E>::new();
    js.h.install_window(&mut js.e);
    js.h.use_virtual_time();
    let g = js.e.global(js.h.realm);
    assert!(js.is(g.get(), "window", g.get()) && js.is(g.get(), "self", g.get()));
    let doc = js.doc();
    assert!(js.is(g.get(), "document", doc.get()));
    let node = js.get(g.get(), "Node");
    assert_eq!(js.read(node.get(), "DOCUMENT_NODE"), "9");
    let (log, other) = (js.e.new_object(js.h.realm), js.e.new_object(js.h.realm));
    let f = js.e.counting(&mut js.h, log.get());
    let f2 = js.e.counting(&mut js.h, other.get());
    let a = js.call(g.get(), "setTimeout", &[f.get(), E::number(20.0)]);
    let b = js.call(g.get(), "setTimeout", &[f2.get(), E::number(10.0)]);
    let c = js.call(g.get(), "setTimeout", &[f.get(), E::number(5.0)]);
    assert!(a.get() != b.get() && js.e.as_number(c.get()).is_some_and(|n| n >= 1.0));
    js.run(g.get(), "clearTimeout", &[c.get()]);
    assert_eq!(js.h.event_loop().pending(), 2);
    assert_eq!(js.h.run_tasks(&mut js.e, 100), 2, "virtual time reaches both");
    assert_eq!((js.read(log.get(), "calls"), js.read(other.get(), "calls")), ("1".into(), "1".into()));
    let i = js.call(g.get(), "setInterval", &[f.get(), E::number(1.0)]);
    assert_eq!(js.h.run_tasks(&mut js.e, 3), 3);
    assert_eq!(js.read(log.get(), "calls"), "4");
    js.run(g.get(), "clearInterval", &[i.get()]);
    assert_eq!(js.h.event_loop().pending(), 0);
    js.run(g.get(), "queueMicrotask", &[f2.get()]);
    // the checkpoint after the call: an engine that drains its queue as
    // the outermost call returns has run it already; the stub, here
    js.e.run_microtasks(&mut js.h);
    assert_eq!(js.read(other.get(), "calls"), "2");
    let r = js.call(g.get(), "queueMicrotask", &[E::number(1.0)]);
    assert_eq!(js.threw(r), "TypeError");
    js.drop_all([i, c, b, a, f2, f, other, log, node, doc, g]);
    let Js { mut e, h } = js;
    h.close(&mut e);
}
