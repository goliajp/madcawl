use madcawl_dom::Dom;
use madcawl_html::{DocumentParser, Options, Source, Step};
use madcawl_str::atom;

use crate::tree::{Inputs, SemTree};

fn parse(html: &str) -> Dom {
    let mut p = DocumentParser::new(Dom::new, Options::default(), Source::default());
    p.feed(html.as_bytes());
    p.end();
    assert_eq!(p.run(), Step::Done);
    p.into_sink()
}

fn by_id(dom: &Dom, id: &str) -> madcawl_dom::NodeId {
    dom.descendants(dom.document())
        .find(|&n| dom.attribute(n, atom!("id")).is_some_and(|v| v.eq_str(id)))
        .expect("the element")
}

#[test]
fn labelledby_entries_go_when_the_element_leaves_is_freed_or_loses_its_role() {
    let mut dom = parse(
        "<span id=l>a</span><input id=i aria-labelledby=l><button id=b aria-labelledby=l>x</button>\
         <button id=c aria-labelledby=l>y</button>",
    );
    let mut t = SemTree::build(&mut dom, Inputs::default(), 1);
    let mut out = Vec::new();
    assert_eq!(t.refs.get("l").map(Vec::len), Some(3));
    let (i, b, c) = (by_id(&dom, "i"), by_id(&dom, "b"), by_id(&dom, "c"));
    // no role: a hidden input
    dom.set_attribute(i, atom!("type"), b"hidden");
    // out of the document, still alive
    dom.remove_node(c);
    assert!(t.sync(&mut dom, Inputs::default(), &mut out));
    assert_eq!(t.refs.get("l").map(Vec::as_slice), Some(&[b][..]));
    assert!(!t.refs_of.contains_key(&i) && !t.refs_of.contains_key(&c));
    // freed
    dom.remove_node(b);
    dom.reclaim();
    assert!(t.sync(&mut dom, Inputs::default(), &mut out));
    assert!(t.refs.is_empty() && t.refs_of.is_empty(), "{:?}", t.refs);
}
