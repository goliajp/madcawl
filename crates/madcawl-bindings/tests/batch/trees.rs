//! Detached trees: what script lets go of is freed before the page
//! closes, and what it holds keeps its whole tree.

use std::hint::black_box;

use madcawl_js::Owned;

use super::{Driver, Js};

/// Make a tree under a removed element, with a text node and an own
/// property on the root, and let go of all of it. Not inlined, so no
/// copy of the values it held is left in the caller's frame for a
/// collector that scans the stack.
#[inline(never)]
fn touch_and_drop<E: Driver>(js: &mut Js<E>, html: E::Value, n: usize) {
    let doc = js.doc();
    for _ in 0..n {
        let div = js.element(doc.get(), "div");
        let span = js.element(doc.get(), "span");
        let s = js.str("x");
        let t = js.call(doc.get(), "createTextNode", &[s.get()]);
        js.run(span.get(), "appendChild", &[t.get()]);
        js.run(div.get(), "appendChild", &[span.get()]);
        js.run(html, "appendChild", &[div.get()]);
        js.set(div.get(), "x", s.get());
        js.run(html, "removeChild", &[div.get()]);
        let first = js.get(span.get(), "firstChild");
        assert!(first.get() == t.get(), "the same wrapper while it is held");
        js.drop_all([first, t, s, span, div]);
    }
    js.drop(doc);
}

/// Overwrite the stack below the caller, where stale copies of dropped
/// values could keep them for a collector that scans it: the engine's
/// own frames under a host call reach deeper than 64 KiB.
#[inline(never)]
fn scrub() {
    let mut pad = [0u64; 64 * 1024];
    for w in pad.iter_mut() {
        // SAFETY: a write to a live local
        unsafe { std::ptr::write_volatile(w, 0) };
    }
    black_box(&pad);
}

fn collect_all<E: Driver>(js: &mut Js<E>) {
    scrub();
    js.e.collect_all(&mut js.h);
}

fn html_in_document<E: Driver>(js: &mut Js<E>) -> madcawl_js::Owned<E::Value> {
    let doc = js.doc();
    let html = js.element(doc.get(), "html");
    js.run(doc.get(), "appendChild", &[html.get()]);
    js.drop(doc);
    html
}

pub fn a_detached_tree_script_let_go_of_is_freed_before_close<E: Driver>() {
    let mut js = Js::<E>::new();
    let html = html_in_document(&mut js);
    let before = js.h.dom.node_count();
    touch_and_drop(&mut js, html.get(), 1);
    collect_all(&mut js);
    assert_eq!(js.h.dom.node_count(), before, "the div, span and text are freed");
    js.audit();
    js.drop(html);
    let Js { mut e, h } = js;
    h.close(&mut e);
}

pub fn holding_one_wrapper_keeps_its_whole_tree<E: Driver>() {
    let mut js = Js::<E>::new();
    let html = html_in_document(&mut js);
    let before = js.h.dom.node_count();
    // what the test holds across a collection is kept on the heap, which
    // a collector that scans the stack does not read
    let mut held = Vec::new();
    remove_a_tree_holding_its_leaf(&mut js, html.get(), &mut held);
    collect_all(&mut js);
    assert_eq!(js.h.dom.node_count(), before + 2);
    put_the_tree_back(&mut js, html.get(), held);
    collect_all(&mut js);
    // freeing it once it is removed again is the script tests' to show:
    // values the test drove through the interface leave copies behind
    // that a collector scanning conservatively may still find
    read_and_remove_last(&mut js, html.get());
    js.audit();
    js.drop(html);
    let Js { mut e, h } = js;
    h.close(&mut e);
}

#[inline(never)]
fn remove_a_tree_holding_its_leaf<E: Driver>(js: &mut Js<E>, html: E::Value, held: &mut Vec<Owned<E::Value>>) {
    let doc = js.doc();
    let div = js.element(doc.get(), "div");
    let span = js.element(doc.get(), "span");
    js.run(div.get(), "appendChild", &[span.get()]);
    js.run(html, "appendChild", &[div.get()]);
    let kept = js.str("kept");
    js.set(div.get(), "x", kept.get());
    js.run(html, "removeChild", &[div.get()]);
    js.drop_all([kept, div, doc]);
    held.push(span);
}

/// The parent's wrapper, with its own property, is the one the
/// collection kept; back in the document, the document keeps it.
#[inline(never)]
fn put_the_tree_back<E: Driver>(js: &mut Js<E>, html: E::Value, mut held: Vec<Owned<E::Value>>) {
    // the buffer goes too: a stale copy of a value there has been seen to
    // keep it from the framework's collector
    let span = held.pop().expect("the leaf");
    drop(held);
    let div = js.get(span.get(), "parentNode");
    assert_eq!(js.read(div.get(), "x"), "kept");
    js.run(html, "appendChild", &[div.get()]);
    js.drop_all([div, span]);
}

#[inline(never)]
fn read_and_remove_last<E: Driver>(js: &mut Js<E>, html: E::Value) {
    let div = js.get(html, "lastChild");
    assert_eq!(js.read(div.get(), "x"), "kept", "the same wrapper, kept by its document");
    js.run(html, "removeChild", &[div.get()]);
    js.drop(div);
}

pub fn the_arena_gives_back_what_a_loop_of_detached_trees_took<E: Driver>() {
    let mut js = Js::<E>::new();
    let html = html_in_document(&mut js);
    let (nodes, pages) = (js.h.dom.node_count(), js.h.dom.used_pages());
    for _ in 0..8 {
        touch_and_drop(&mut js, html.get(), 250);
        collect_all(&mut js);
    }
    assert_eq!(js.h.dom.node_count(), nodes);
    assert!(js.h.dom.used_pages() <= pages + 1, "{} pages after, {pages} before", js.h.dom.used_pages());
    js.audit();
    js.drop(html);
    let Js { mut e, h } = js;
    h.close(&mut e);
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
