//! Script between two unlink batches of a collection adds a listener
//! to a node whose old wrapper is being unlinked: the listener belongs
//! to the node, not to the garbage, and outlives the collection.

mod support;

use madcawl_js::{Deadline, Engine, Step};
use madcawl_js_stub::Stub;
use madcawl_str::atom;
use support::{Js, V};

#[test]
fn a_listener_added_between_unlink_batches_outlives_the_collection() {
    let mut js = Js::new();
    let html = js.connected_root();
    let div = js.element("div");
    js.run(html.get(), "appendChild", &[div.get()]);
    js.collect();
    // two garbage pairs of plain objects, the second holding the div's
    // wrapper, so that the wrapper is unlinked after the first batch
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
        assert_eq!(js.h.collect(&mut js.e, Deadline::Work(1)), Step::More);
    }
    let again = js.get(html.get(), "firstChild");
    assert_ne!(Stub::wrapper_word(again.get()), old, "a new wrapper, not the condemned one");
    let log = js.object();
    let l = js.retain(log.get());
    let f = js.closure(vec![l]);
    let click = js.str("click");
    js.run(again.get(), "addEventListener", &[click.get(), f.get()]);
    let stats = js.collect();
    assert_eq!(stats.survivors, 0);
    assert_eq!(js.h.listener_count(n), 1, "the listener is the node's");
    assert_eq!(js.h.fire(&mut js.e, n, atom!("click")), 1);
    assert_eq!(js.number(log.get(), "calls"), 1.0);
    for v in [click, f, log, again, html] {
        js.drop(v);
    }
    js.audit();
}
