//! Every way script makes a wrapper observable preserves it: an own
//! property, use as a WeakMap key or a WeakRef target, a changed
//! prototype, freezing, preventing extensions. Each goes through the
//! same round: script lets go of the wrapper while its node stays in
//! the document, a collection runs, and script reads the node again and
//! finds the same wrapper with its state. Then the node leaves the
//! document and everything is freed.

mod support;

use madcawl_js::Engine;
use madcawl_js_stub::{Stub, Value};
use support::{Js, V};

/// Makes wrapper `w` observable; what script keeps to look at it with.
type Trigger = fn(&mut Js, Value) -> Vec<V>;
/// Checks wrapper `again` against what the trigger kept.
type Check = fn(&mut Js, Value, &[V]);

fn round(name: &str, trigger: Trigger, check: Check) {
    let mut js = Js::new();
    let html = js.connected_root();
    let base = js.h.dom.node_count();
    let div = js.element("div");
    js.run(html.get(), "appendChild", &[div.get()]);
    let word = Stub::wrapper_word(div.get());
    let kept = trigger(&mut js, div.get());
    js.drop(div);
    js.collect();
    let again = js.get(html.get(), "firstChild");
    assert_eq!(Stub::wrapper_word(again.get()), word, "{name}: the same wrapper");
    check(&mut js, again.get(), &kept);
    js.run(html.get(), "removeChild", &[again.get()]);
    for v in kept.into_iter().chain([again]) {
        js.drop(v);
    }
    let stats = js.collect();
    assert_eq!(stats.survivors, 0, "{name}");
    assert_eq!(js.h.dom.node_count(), base, "{name}: the div is freed once out of the document");
    js.audit();
    js.drop(html);
}

#[test]
fn an_own_property_preserves_the_wrapper() {
    round(
        "own property",
        |js, w| {
            let tag = js.object();
            js.set(w, "expando", tag.get());
            vec![tag]
        },
        |js, w, kept| assert!(js.is(w, "expando", kept[0].get())),
    );
}

#[test]
fn a_weak_map_key_preserves_the_wrapper() {
    round(
        "weak map key",
        |js, w| {
            let map = js.e.new_weak_map(js.h.realm);
            let value = js.object();
            js.e.weak_map_set(&mut js.h, map.get(), w, value.get());
            vec![map, value]
        },
        |js, w, kept| {
            let v = js.e.weak_map_get(kept[0].get(), w);
            assert_eq!(v.get(), kept[1].get(), "the entry is found by the wrapper read again");
            js.drop(v);
        },
    );
}

#[test]
fn a_weak_ref_target_preserves_the_wrapper() {
    round(
        "weak ref target",
        |js, w| {
            let realm = js.h.realm;
            let r = js.e.new_weak_ref(&mut js.h, realm, w);
            vec![r]
        },
        |js, w, kept| {
            let v = js.e.weak_ref_deref(kept[0].get());
            assert_eq!(v.get(), w, "deref gives the wrapper script reads again");
            js.drop(v);
        },
    );
}

#[test]
fn a_changed_prototype_preserves_the_wrapper() {
    round(
        "prototype",
        |js, w| {
            let proto = js.object();
            assert!(js.e.set_prototype(&mut js.h, w, proto.get()));
            vec![proto]
        },
        |js, w, kept| {
            let p = js.e.prototype_of::<madcawl_bindings::DomHost<Stub>>(w);
            assert_eq!(p.get(), kept[0].get());
            js.drop(p);
        },
    );
}

#[test]
fn freezing_preserves_the_wrapper() {
    round(
        "frozen",
        |js, w| {
            js.e.freeze(&mut js.h, w);
            vec![]
        },
        |js, w, _| {
            assert!(js.e.is_frozen(w));
            let x = js.object();
            js.set(w, "late", x.get());
            let late = js.get(w, "late");
            assert_eq!(late.get(), Stub::undefined(), "no property added");
            for v in [late, x] {
                js.drop(v);
            }
        },
    );
}

#[test]
fn preventing_extensions_preserves_the_wrapper() {
    round(
        "not extensible",
        |js, w| {
            js.e.prevent_extensions(&mut js.h, w);
            vec![]
        },
        |js, w, _| assert!(!js.e.is_extensible(w)),
    );
}

#[test]
fn a_weak_handle_of_the_host_does_not_preserve_the_wrapper() {
    let mut js = Js::new();
    let html = js.connected_root();
    let div = js.element("div");
    js.run(html.get(), "appendChild", &[div.get()]);
    let weak = js.e.weak(div.get());
    let word = Stub::wrapper_word(div.get());
    js.drop(div);
    assert!(js.gone(&weak), "the wrapper died with the node alive");
    let again = js.get(html.get(), "firstChild");
    assert_ne!(Stub::wrapper_word(again.get()), word);
    js.e.weak_drop(weak);
    for v in [again, html] {
        js.drop(v);
    }
}
