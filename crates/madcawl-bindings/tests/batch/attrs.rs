//! Attributes on every engine: those of a parsed document, and
//! attributes as `Attr` objects, one object per attribute, its value
//! read from and written to the element, taken off and put back, and an
//! `Attr` that holds its element.

use super::{Driver, Js};

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

pub fn an_attr_is_the_same_object_and_follows_its_element<E: Driver>() {
    let mut js = Js::<E>::parsed("<p id=a title=t>x</p>");
    let doc = js.doc();
    let id = js.str("a");
    let p = js.call(doc.get(), "getElementById", &[id.get()]);
    let title = js.str("title");
    let t = js.call(p.get(), "getAttributeNode", &[title.get()]);
    let again = js.call(p.get(), "getAttributeNode", &[title.get()]);
    assert!(t.get() == again.get(), "one Attr per attribute");
    let map = js.get(p.get(), "attributes");
    assert!(js.is(p.get(), "attributes", map.get()), "the same map");
    assert_eq!(js.read(map.get(), "length"), "2");
    let named = js.call(map.get(), "getNamedItem", &[title.get()]);
    let second = js.call(map.get(), "item", &[E::number(1.0)]);
    assert!(named.get() == t.get() && second.get() == t.get());
    assert_eq!((js.read(t.get(), "name"), js.read(t.get(), "value"), js.read(t.get(), "nodeType")), ("title".into(), "t".into(), "2".into()));
    assert!(js.is(t.get(), "ownerElement", p.get()));
    let u = js.str("u");
    js.set(t.get(), "value", u.get());
    let v = js.call(p.get(), "getAttribute", &[title.get()]);
    assert_eq!(js.text(v.get()), "u");
    js.run(p.get(), "removeAttribute", &[title.get()]);
    assert_eq!((js.read(t.get(), "ownerElement"), js.read(t.get(), "value"), js.read(map.get(), "length")), ("null".into(), "u".into(), "1".into()));
    let made = js.str("Data-X");
    let c = js.call(doc.get(), "createAttribute", &[made.get()]);
    assert_eq!(js.read(c.get(), "name"), "data-x");
    let old = js.call(p.get(), "setAttributeNode", &[t.get()]);
    assert!(old.get() == E::null());
    assert!(js.is(t.get(), "ownerElement", p.get()));
    let other = js.element(doc.get(), "q");
    let r = js.call(other.get(), "setAttributeNode", &[t.get()]);
    assert_eq!(js.threw(r), "InUseAttributeError");
    let gone = js.call(p.get(), "removeAttributeNode", &[t.get()]);
    assert!(gone.get() == t.get());
    let r = js.call(p.get(), "removeAttributeNode", &[t.get()]);
    assert_eq!(js.threw(r), "NotFoundError");
    js.audit();
    js.drop_all([gone, other, old, c, made, v, u, second, named, map, again, t, title, p, id, doc]);
    let Js { mut e, h } = js;
    h.close(&mut e);
}

/// An `Attr` script keeps holds its element after script let go of the
/// element; dropping the `Attr` frees both. Script values stay out of
/// this function's frame, which a conservative collector would scan:
/// the `Attr` is kept on a plain object.
pub fn an_attr_holds_its_element_until_it_is_dropped<E: Driver>() {
    let mut js = Js::<E>::new();
    let keep = js.e.new_object(js.h.realm);
    super::trees::scrub();
    js.e.collect_all(&mut js.h);
    let (nodes, sides) = (js.h.dom.node_count(), js.h.side_objects());
    keep_the_attr_of_a_dropped_element(&mut js, keep.get());
    super::trees::scrub();
    js.e.collect_all(&mut js.h);
    assert_eq!(js.h.dom.node_count(), nodes + 1, "the element is held");
    read_and_drop_the_attr(&mut js, keep.get());
    super::trees::scrub();
    js.e.collect_all(&mut js.h);
    assert_eq!((js.h.dom.node_count(), js.h.side_objects()), (nodes, sides));
    js.audit();
    js.drop(keep);
    let Js { mut e, h } = js;
    h.close(&mut e);
}

#[inline(never)]
fn keep_the_attr_of_a_dropped_element<E: Driver>(js: &mut Js<E>, keep: E::Value) {
    let doc = js.doc();
    let div = js.element(doc.get(), "div");
    let (a, one) = (js.str("a"), js.str("1"));
    js.run(div.get(), "setAttribute", &[a.get(), one.get()]);
    let attr = js.call(div.get(), "getAttributeNode", &[a.get()]);
    js.set(keep, "attr", attr.get());
    js.drop_all([attr, one, a, div, doc]);
}

#[inline(never)]
fn read_and_drop_the_attr<E: Driver>(js: &mut Js<E>, keep: E::Value) {
    let attr = js.get(keep, "attr");
    let el = js.get(attr.get(), "ownerElement");
    assert_eq!(js.read(el.get(), "tagName"), "DIV");
    js.set(keep, "attr", E::undefined());
    js.drop_all([el, attr]);
}

/// `setAttribute` by a qualified name changes the parser's namespaced
/// attribute of that name, not a second one beside it.
pub fn set_attribute_by_qualified_name_finds_a_namespaced_attribute<E: Driver>() {
    let mut js = Js::<E>::parsed("<svg><a xlink:href=u></a></svg>");
    let doc = js.doc();
    let sel = js.str("a");
    let a = js.call(doc.get(), "querySelector", &[sel.get()]);
    let (name, v) = (js.str("xlink:href"), js.str("v"));
    let attr = js.call(a.get(), "getAttributeNode", &[name.get()]);
    js.run(a.get(), "setAttribute", &[name.get(), v.get()]);
    let names = js.call(a.get(), "getAttributeNames", &[]);
    assert_eq!(js.read(names.get(), "length"), "1", "one attribute");
    assert_eq!((js.read(attr.get(), "value"), js.read(attr.get(), "namespaceURI")), ("v".into(), "http://www.w3.org/1999/xlink".into()));
    let upper = js.str("XLINK:HREF");
    js.run(a.get(), "setAttribute", &[upper.get(), v.get()]);
    let names2 = js.call(a.get(), "getAttributeNames", &[]);
    assert_eq!(js.read(names2.get(), "length"), "2", "names are not lowercased outside HTML");
    js.audit();
    js.drop_all([names2, upper, names, attr, v, name, a, sel, doc]);
    let Js { mut e, h } = js;
    h.close(&mut e);
}
