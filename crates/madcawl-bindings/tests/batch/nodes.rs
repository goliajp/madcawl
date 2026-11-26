//! The node interfaces beyond Element and Document: character data and
//! fragments, read and edited through their own members.

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
