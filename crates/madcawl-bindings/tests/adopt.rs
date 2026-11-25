//! The decrement of a node's old document when the node changes
//! document: moving a subtree out of a template's contents (owned by
//! the inert document) into the main document, and the other way. The
//! old document is the main document or its inert document, which the
//! main document holds for its whole life, so the decrement never
//! leaves a document only in a cycle; what is left in a cycle is the
//! moved subtree, and the counts on both documents must come back.

mod support;

use support::Js;

/// A template, an `<i>` with a `<b>` in its contents; the template's
/// wrapper and the node of the `<i>`.
fn template_with_contents(js: &mut Js) -> (support::V, madcawl_dom::NodeId) {
    let t = js.element("template");
    let contents = js.h.dom.contents(js.node(t.get())).unwrap();
    let i = js.h.dom.new_element(b"i").unwrap();
    let b = js.h.dom.new_element(b"b").unwrap();
    js.h.dom.insert_node(contents, i, None);
    js.h.dom.insert_node(i, b, None);
    js.h.settle(&mut js.e);
    (t, i)
}

#[test]
fn a_subtree_moved_out_of_template_contents_changes_document_and_is_freed() {
    let mut js = Js::new();
    let doc = js.h.dom.document();
    let start = js.h.dom.ref_count(doc);
    let (t, i) = template_with_contents(&mut js);
    let inert = js.h.dom.owner_document(i);
    assert!(js.h.dom.is_inert_document(inert));
    // the template, its contents, the inert document, i and b
    let base = js.h.dom.node_count() - 5;
    let inert_count = js.h.dom.ref_count(inert);
    let p = js.element("div");
    let iw = js.wrap(i);
    js.run(p.get(), "appendChild", &[iw.get()]);
    assert_eq!(js.h.dom.owner_document(i), doc);
    assert_eq!(js.h.dom.ref_count(inert), inert_count - 2, "i and b gave up the inert document");
    for v in [iw, p, t] {
        js.drop(v);
    }
    // p, i and b hold each other: only the collector frees them
    let stats = js.collect();
    assert_eq!(stats.survivors, 0);
    assert_eq!(js.h.dom.node_count(), base + 1, "only the inert document is left");
    assert_eq!(js.h.dom.ref_count(doc), start, "the main document's count is back");
    assert_eq!(js.h.dom.ref_count(inert), 1, "the main document alone holds the inert one");
    js.audit();
}

#[test]
fn a_subtree_moved_into_template_contents_changes_document_and_is_freed() {
    let mut js = Js::new();
    let (t, i) = template_with_contents(&mut js);
    let inert = js.h.dom.owner_document(i);
    let base = js.h.dom.node_count() - 5;
    let p = js.element("div");
    let c = js.element("span");
    js.run(p.get(), "appendChild", &[c.get()]);
    let iw = js.wrap(i);
    let before = js.h.dom.ref_count(js.h.dom.document());
    // the detached p moves under i, into the contents
    js.run(iw.get(), "appendChild", &[p.get()]);
    assert_eq!(js.h.dom.owner_document(js.node(c.get())), inert);
    assert_eq!(js.h.dom.ref_count(js.h.dom.document()), before - 2, "p and c gave up the main document");
    for v in [iw, c, p, t] {
        js.drop(v);
    }
    let stats = js.collect();
    assert_eq!(stats.survivors, 0);
    assert_eq!(js.h.dom.node_count(), base + 1, "only the inert document is left");
    assert_eq!(js.h.dom.ref_count(inert), 1, "the main document alone holds the inert one");
    js.audit();
}
