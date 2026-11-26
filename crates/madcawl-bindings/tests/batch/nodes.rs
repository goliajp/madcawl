//! The node interfaces beyond Element and Document: character data and
//! fragments, read and edited through their own members; collections.

use madcawl_js::Key;

use super::{Driver, Js};

pub fn character_data_is_edited_by_code_unit<E: Driver>() {
    let mut js = Js::<E>::new();
    let doc = js.doc();
    let s = js.str("h\u{e9}llo");
    let t = js.call(doc.get(), "createTextNode", &[s.get()]);
    assert_eq!(js.read(t.get(), "length"), "5");
    let tail = js.str(" \u{3042}");
    js.run(t.get(), "appendData", &[tail.get()]);
    let (one, two) = (E::number(1.0), E::number(2.0));
    js.run(t.get(), "deleteData", &[one, two]);
    assert_eq!(js.read(t.get(), "data"), "hlo \u{3042}");
    let r = js.call(t.get(), "substringData", &[E::number(9.0), one]);
    assert_eq!(js.threw(r), "IndexSizeError");
    let f = js.call(doc.get(), "createDocumentFragment", &[]);
    assert!(!E::is_thrown(f.get()));
    assert_eq!(js.read(f.get(), "nodeType"), "11");
    js.run(f.get(), "appendChild", &[t.get()]);
    let first = js.get(f.get(), "firstChild");
    assert!(first.get() == t.get());
    js.drop_all([first, f, tail, t, s, doc]);
    js.audit();
}

pub fn collections_are_live_and_child_lists_the_same_object<E: Driver>() {
    let mut js = Js::<E>::new();
    let sides = js.h.side_objects();
    read_collections(&mut js);
    super::trees::scrub();
    js.e.collect_all(&mut js.h);
    js.h.settle(&mut js.e);
    assert_eq!(js.h.side_objects(), sides, "dropped collections are freed");
    js.audit();
}

#[inline(never)]
fn read_collections<E: Driver>(js: &mut Js<E>) {
    let doc = js.doc();
    let div = js.element(doc.get(), "div");
    let (a, b) = (js.element(doc.get(), "span"), js.element(doc.get(), "i"));
    let t = js.str("x");
    let text = js.call(doc.get(), "createTextNode", &[t.get()]);
    for n in [&a, &text, &b] {
        js.run(div.get(), "appendChild", &[n.get()]);
    }
    let nodes = js.get(div.get(), "childNodes");
    let kids = js.get(div.get(), "children");
    let name = js.str("span");
    let spans = js.call(div.get(), "getElementsByTagName", &[name.get()]);
    assert_eq!((js.read(nodes.get(), "length"), js.read(kids.get(), "length"), js.read(spans.get(), "length")), ("3".into(), "2".into(), "1".into()));
    assert!(js.is(div.get(), "childNodes", nodes.get()), "the same NodeList each time");
    let second = js.e.get(&mut js.h, kids.get(), Key::Index(1));
    assert!(second.get() == b.get());
    let past = js.e.get(&mut js.h, nodes.get(), Key::Index(3));
    assert!(past.get() == E::undefined());
    let c = js.element(doc.get(), "span");
    js.run(div.get(), "appendChild", &[c.get()]);
    assert_eq!((js.read(nodes.get(), "length"), js.read(spans.get(), "length")), ("4".into(), "2".into()));
    let one = E::number(1.0);
    let found = js.call(spans.get(), "item", &[one]);
    assert!(found.get() == c.get());
    js.drop_all([found, past, second, c, spans, name, kids, nodes, text, t, b, a, div, doc]);
}
