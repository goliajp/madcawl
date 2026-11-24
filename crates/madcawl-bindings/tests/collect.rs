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

#[test]
fn a_garbage_wrapper_read_between_unlink_batches_keeps_what_script_writes() {
    let mut js = Js::new();
    let html = connected_root(&mut js);
    let div = element(&mut js, "div");
    js.run(html.get(), "appendChild", &[div.get()]);
    // empty the candidates, so that the wrapper is unlinked last
    js.collect();
    // two garbage pairs of plain objects, the second of which holds the
    // div's wrapper; the div is in the document. Unlinking the first pair
    // frees only that pair, so between batches the wrapper is still held
    // and is still the div's
    let pairs: Vec<V> = (0..4).map(|_| js.object()).collect();
    for k in [0, 2] {
        js.set(pairs[k].get(), "next", pairs[k + 1].get());
        js.set(pairs[k + 1].get(), "next", pairs[k].get());
    }
    js.set(pairs[3].get(), "w", div.get());
    let n = js.node(div.get());
    let old = js.h.dom.wrapper(n);
    for v in pairs {
        js.drop(v);
    }
    js.drop(div);
    while !js.e.unlinking() {
        assert_eq!(js.h.collect(&mut js.e, Deadline::Work(1)), madcawl_js::Step::More);
    }
    // script reads the div again and gives it an expando
    let again = js.get(html.get(), "firstChild");
    assert_ne!(Stub::wrapper_word(again.get()), old, "a new wrapper, not the condemned one");
    let tag = js.object();
    js.set(again.get(), "expando", tag.get());
    let stats = js.collect();
    assert!(js.is(again.get(), "expando", tag.get()), "the expando outlives the collection");
    assert_eq!(stats.survivors, 0);
    for v in [tag, again, html] {
        js.drop(v);
    }
    js.audit();
}

#[test]
fn a_parent_left_in_a_cycle_by_moving_a_child_out_is_freed() {
    let mut js = Js::new();
    let html = connected_root(&mut js);
    let base = js.h.dom.node_count();
    let p = element(&mut js, "div");
    let a = element(&mut js, "a");
    let b = element(&mut js, "b");
    js.run(p.get(), "appendChild", &[a.get()]);
    js.run(p.get(), "appendChild", &[b.get()]);
    js.drop(b);
    js.drop(p);
    // a, which script holds, holds its parent: nothing is garbage yet
    assert_eq!(js.collect().garbage, 0);
    // moving a out leaves p and b holding only each other
    js.run(html.get(), "appendChild", &[a.get()]);
    js.collect();
    assert_eq!(js.h.dom.node_count(), base + 1, "only a is left");
    js.drop(a);
    js.drop(html);
    js.audit();
}

#[test]
fn a_template_whose_contents_have_children_is_freed_with_them() {
    let mut js = Js::new();
    let base = js.h.dom.node_count();
    let t = element(&mut js, "template");
    let contents = js.h.dom.contents(js.node(t.get())).unwrap();
    let i = js.h.dom.new_element(b"i").unwrap();
    js.h.dom.insert_node(contents, i, None);
    js.h.settle(&mut js.e);
    // the template, its contents, their owner document and the child
    assert_eq!(js.h.dom.node_count(), base + 4);
    js.drop(t);
    let stats = js.collect();
    assert_eq!(stats.survivors, 0);
    assert_eq!(js.h.dom.node_count(), base + 1, "the inert document stays");
    js.audit();
}

#[test]
fn a_dead_wrapper_read_before_its_notice_is_replaced() {
    let mut js = Js::new();
    let html = connected_root(&mut js);
    let div = element(&mut js, "div");
    js.run(html.get(), "appendChild", &[div.get()]);
    let n = js.node(div.get());
    let old = js.h.dom.wrapper(n);
    // the count reaches 0 and the notice waits for the host
    js.e.release(div);
    assert_eq!(js.h.dom.wrapper(n), old);
    let again = js.get(html.get(), "firstChild");
    let new = Stub::wrapper_word(again.get());
    assert_ne!(new, old, "a new wrapper, not the dead one");
    js.h.settle(&mut js.e);
    assert_eq!(js.h.dom.wrapper(n), new, "the old wrapper's end clears nothing");
    js.audit();
    js.drop(again);
    assert_eq!(js.h.dom.wrapper(n), 0);
    js.drop(html);
    js.audit();
}

#[test]
#[should_panic(expected = "a wrapper for a host object found to be garbage")]
fn a_wrapper_for_a_garbage_node_reached_by_its_raw_id_between_batches_is_refused() {
    let mut js = Js::new();
    js.collect();
    // a garbage pair of plain objects comes first, so that the first
    // unlink batch leaves the nodes alone
    let pair = [js.object(), js.object()];
    js.set(pair[0].get(), "next", pair[1].get());
    js.set(pair[1].get(), "next", pair[0].get());
    for v in pair {
        js.drop(v);
    }
    // a detached parent and child hold each other: both are garbage
    let p = element(&mut js, "div");
    let c = element(&mut js, "i");
    js.run(p.get(), "appendChild", &[c.get()]);
    let id = js.node(c.get());
    js.drop(c);
    js.drop(p);
    js.h.settle(&mut js.e);
    while !js.e.unlinking() {
        assert_eq!(js.h.collect(&mut js.e, Deadline::Work(1)), madcawl_js::Step::More);
    }
    // the raw id is counted by nothing: a wrapper made from it would be
    // unlinked with the node
    let _ = js.h.wrap(&mut js.e, id);
}
