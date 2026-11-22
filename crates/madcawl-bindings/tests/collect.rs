//! The engine's cycle collector over the DOM: every counted edge of a
//! node, listeners on nodes with and without wrappers, member, root and
//! weak handles, and preserved wrappers.

mod support;

use std::collections::HashMap;

use madcawl_bindings::NodeOps;
use madcawl_dom::{Dom, NodeId};
use madcawl_html::{Options, Step, Tokenizer, TreeBuilder};
use madcawl_js::{Deadline, Engine, Root};
use madcawl_js_stub::{Stub, StubTracer};
use madcawl_str::atom;
use support::{Js, V};

/// `document.createElement(name)`.
fn element(js: &mut Js, name: &str) -> V {
    let doc = js.doc();
    let n = js.str(name);
    let el = js.call(doc.get(), "createElement", &[n.get()]);
    js.drop(n);
    js.drop(doc);
    el
}

/// An `<html>` in the document, the parent connected nodes go under.
fn connected_root(js: &mut Js) -> V {
    let doc = js.doc();
    let html = element(js, "html");
    js.run(doc.get(), "appendChild", &[html.get()]);
    js.drop(doc);
    html
}

/// A listener on `target` that closes over `captured`, added through
/// `addEventListener`; the closure is returned with the test's count.
fn listen(js: &mut Js, target: &V, captured: &V) -> V {
    let log = js.object();
    let c = js.retain(captured.get());
    let f = js.closure(vec![log, c]);
    let click = js.str("click");
    js.run(target.get(), "addEventListener", &[click.get(), f.get()]);
    js.drop(click);
    f
}

#[test]
fn a_detached_subtree_without_wrappers_is_freed() {
    let mut js = Js::new();
    let base = js.h.dom.node_count();
    let div = element(&mut js, "div");
    let span = element(&mut js, "span");
    js.run(div.get(), "appendChild", &[span.get()]);
    let t = js.str("text");
    js.set(span.get(), "textContent", t.get());
    js.drop(t);
    js.drop(span);
    js.drop(div);
    // parent and child hold each other: only the collector frees them
    assert_eq!(js.h.dom.node_count(), base + 3);
    let stats = js.collect();
    assert_eq!(stats.garbage, 3);
    assert_eq!(js.h.dom.node_count(), base);
    assert_eq!(js.h.dom.ref_count(js.h.dom.document()), 0, "their edges to the document went too");
    js.audit();
}

#[test]
fn a_listener_that_closes_over_its_node_is_freed_with_it() {
    let mut js = Js::new();
    let base = js.h.dom.node_count();
    let div = element(&mut js, "div");
    let f = listen(&mut js, &div, &div);
    let (wf, wd) = (js.e.weak(f.get()), js.e.weak(div.get()));
    js.drop(f);
    js.drop(div);
    assert!(!js.gone(&wf) && !js.gone(&wd), "wrapper, node and closure hold each other");
    let stats = js.collect();
    assert_eq!(stats.survivors, 0);
    assert!(js.gone(&wf) && js.gone(&wd));
    assert_eq!(js.h.dom.node_count(), base);
    js.audit();
}

#[test]
fn a_listener_on_a_node_without_a_wrapper_is_in_the_graph() {
    let mut js = Js::new();
    let base = js.h.dom.node_count();
    let div = element(&mut js, "div");
    let span = element(&mut js, "span");
    js.run(div.get(), "appendChild", &[span.get()]);
    let f = listen(&mut js, &span, &div);
    let span_node = js.node(span.get());
    js.drop(span);
    assert_eq!(js.h.dom.wrapper(span_node), 0, "the span lost its wrapper and kept its listener");
    assert_eq!(js.h.listener_count(span_node), 1);
    let wf = js.e.weak(f.get());
    js.drop(f);
    js.drop(div);
    js.collect();
    assert!(js.gone(&wf));
    assert_eq!(js.h.dom.node_count(), base);
    js.audit();
}

#[test]
fn the_same_cycle_in_the_document_is_skipped_and_still_works() {
    let mut js = Js::new();
    let html = connected_root(&mut js);
    let div = element(&mut js, "div");
    let span = element(&mut js, "span");
    js.run(div.get(), "appendChild", &[span.get()]);
    js.run(html.get(), "appendChild", &[div.get()]);
    let f = listen(&mut js, &span, &div);
    let span_node = js.node(span.get());
    let count = js.h.dom.node_count();
    for v in [f, span, div, html] {
        js.drop(v);
    }
    js.collect();
    assert_eq!(js.h.dom.node_count(), count);
    assert_eq!(js.h.fire(&mut js.e, span_node, atom!("click")), 1);
    js.audit();
}

#[test]
fn a_root_handle_keeps_the_cycle_until_it_is_released() {
    let mut js = Js::new();
    let base = js.h.dom.node_count();
    let div = element(&mut js, "div");
    let f = listen(&mut js, &div, &div);
    // a timer holds the callback
    let timer = Root::new(js.retain(f.get()));
    js.drop(f);
    js.drop(div);
    js.collect();
    assert_eq!(js.h.dom.node_count(), base + 1);
    timer.release(&mut js.e);
    js.h.settle(&mut js.e);
    js.collect();
    assert_eq!(js.h.dom.node_count(), base);
    js.audit();
}

#[test]
fn an_own_property_preserves_the_wrapper_while_the_node_lives() {
    let mut js = Js::new();
    let html = connected_root(&mut js);
    let div = element(&mut js, "div");
    js.run(html.get(), "appendChild", &[div.get()]);
    let n = js.node(div.get());
    let word = js.h.dom.wrapper(n);
    let obj = js.object();
    let wobj = js.e.weak(obj.get());
    js.set(div.get(), "data", obj.get());
    js.drop(obj);
    assert_eq!(js.h.dom.wrapper(n), word | 1, "preserved");
    js.drop(div);
    js.collect();
    let again = js.get(html.get(), "firstChild");
    assert_eq!(Stub::wrapper_word(again.get()), word, "the same wrapper");
    assert!(!js.gone(&wobj), "with its property");
    // out of the document, the node, its preserved wrapper and the
    // property form a cycle through the preserved edge
    let removed = js.call(html.get(), "removeChild", &[again.get()]);
    js.drop(removed);
    js.drop(again);
    let stats = js.collect();
    assert_eq!((stats.garbage, stats.survivors), (3, 0), "the node, its wrapper, the property");
    assert!(js.gone(&wobj));
    js.drop(html);
    js.audit();
}

#[test]
fn a_wrapper_without_state_is_dropped_and_made_anew() {
    let mut js = Js::new();
    let html = connected_root(&mut js);
    let div = element(&mut js, "div");
    js.run(html.get(), "appendChild", &[div.get()]);
    let n = js.node(div.get());
    let cells = js.e.live_cells();
    js.drop(div);
    assert_eq!(js.h.dom.wrapper(n), 0);
    assert_eq!(js.e.live_cells(), cells - 1);
    let again = js.get(html.get(), "firstChild");
    assert_eq!(js.node(again.get()), n);
    js.drop(again);
    js.drop(html);
    js.audit();
}

#[test]
fn a_template_goes_with_its_contents_and_the_inert_document_stays() {
    let mut js = Js::new();
    let base = js.h.dom.node_count();
    let t = element(&mut js, "template");
    let f = listen(&mut js, &t, &t);
    // the template, its contents fragment, and the contents owner
    // document the main document now holds
    assert_eq!(js.h.dom.node_count(), base + 3);
    js.drop(f);
    js.drop(t);
    let stats = js.collect();
    assert_eq!(stats.survivors, 0);
    assert_eq!(js.h.dom.node_count(), base + 1);
    js.audit();
}

#[test]
fn a_tree_put_back_between_batches_is_not_freed() {
    let mut js = Js::new();
    let html = connected_root(&mut js);
    let root = element(&mut js, "div");
    for _ in 0..40 {
        let c = element(&mut js, "p");
        js.run(root.get(), "appendChild", &[c.get()]);
        js.drop(c);
    }
    let (root_node, html_node) = (js.node(root.get()), js.node(html.get()));
    let count = js.h.dom.node_count();
    js.drop(root);
    assert_eq!(js.h.collect(&mut js.e, Deadline::Work(5)), madcawl_js::Step::More);
    // a task puts the tree back before the collection is done
    js.h.dom.insert_node(html_node, root_node, None);
    let stats = js.collect();
    assert_eq!(stats.garbage, 0);
    assert!(stats.batches > 2);
    assert_eq!(js.h.dom.node_count(), count);
    js.h.dom.remove_node(root_node);
    js.h.settle(&mut js.e);
    js.collect();
    assert_eq!(js.h.dom.node_count(), count - 41);
    js.drop(html);
    js.audit();
}

#[test]
fn nodes_in_the_document_are_not_entered() {
    let mut js = Js::new();
    let html = connected_root(&mut js);
    for _ in 0..200 {
        let c = element(&mut js, "p");
        js.run(html.get(), "appendChild", &[c.get()]);
        js.drop(c);
    }
    let div = element(&mut js, "div");
    // the detached div holds its document, which holds 200 nodes
    let f = listen(&mut js, &div, &div);
    js.drop(f);
    js.drop(div);
    let stats = js.collect();
    assert_eq!(stats.garbage, 4, "the div, its wrapper, the closure, the object it closes over");
    // besides those, only the other candidates (method functions, the
    // wrapper of <html>): none of the 200 nodes
    assert!(stats.visited < 12, "{stats:?}");
    assert!(stats.skipped >= 1);
    js.drop(html);
}

#[test]
fn traverse_reports_each_counted_edge_once() {
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(Dom::new(), Options::default());
    tok.feed(b"<!doctype html><p>a<b>c</b><template><i>x</i></template><!-- c -->");
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    let mut js = Js::with(b.into_sink());
    let doc = js.doc();
    let html = js.get(doc.get(), "documentElement");
    let f = listen(&mut js, &html, &doc);
    js.set(html.get(), "x", f.get());
    let mut nodes: Vec<NodeId> = vec![js.h.dom.document()];
    let mut i = 0;
    while i < nodes.len() {
        let n = nodes[i];
        nodes.extend(js.h.dom.children(n));
        nodes.extend(js.h.dom.contents(n));
        i += 1;
    }
    let template = *nodes.iter().find(|&&n| js.h.dom.contents(n).is_some()).unwrap();
    nodes.push(js.h.dom.owner_document(js.h.dom.contents(template).unwrap()));
    let mut reported: HashMap<usize, u32> = HashMap::new();
    let mut values = 0;
    for &n in &nodes {
        let mut t = StubTracer::default();
        (NodeOps::<Stub>::OPS.traverse)(&js.h, js.h.host_ref(n), &mut t);
        for h in t.hosts() {
            *reported.entry(h.addr()).or_default() += 1;
        }
        values += t.values().count();
    }
    for &n in &nodes {
        // the wrapper's edge to its node is the engine's to report
        let wrapper = (js.h.dom.wrapper(n) != 0) as u32;
        let addr = js.h.dom.address(n);
        assert_eq!(
            reported.get(&addr).copied().unwrap_or(0) + wrapper,
            js.h.dom.ref_count(n),
            "{n:?} {:?}",
            js.h.dom.kind(n)
        );
    }
    assert_eq!(values, 2, "the listener and the preserved wrapper of <html>");
    for v in [f, html, doc] {
        js.drop(v);
    }
    js.audit();
}
