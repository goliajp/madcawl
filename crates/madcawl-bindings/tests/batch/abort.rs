//! The event loop's timers, and abort signals as `AbortSignal`'s static
//! operations make them (a controller needs `new`, which only script
//! has): their reason, their event handler, and the listeners added
//! with them.

use madcawl_js::ValueKind;
use madcawl_str::atom;

use super::{Driver, Js};

pub fn timers_and_microtasks_run_on_the_event_loop<E: Driver>() {
    let mut js = Js::<E>::new();
    js.h.install_window(&mut js.e);
    js.h.use_virtual_time();
    let g = js.e.global(js.h.realm);
    assert!(js.is(g.get(), "window", g.get()) && js.is(g.get(), "self", g.get()));
    let doc = js.doc();
    assert!(js.is(g.get(), "document", doc.get()));
    let node = js.get(g.get(), "Node");
    assert_eq!(js.read(node.get(), "DOCUMENT_NODE"), "9");
    let (log, other) = (js.e.new_object(js.h.realm), js.e.new_object(js.h.realm));
    let f = js.e.counting(&mut js.h, log.get());
    let f2 = js.e.counting(&mut js.h, other.get());
    let a = js.call(g.get(), "setTimeout", &[f.get(), E::number(20.0)]);
    let b = js.call(g.get(), "setTimeout", &[f2.get(), E::number(10.0)]);
    let c = js.call(g.get(), "setTimeout", &[f.get(), E::number(5.0)]);
    assert!(a.get() != b.get() && js.e.as_number(c.get()).is_some_and(|n| n >= 1.0));
    js.run(g.get(), "clearTimeout", &[c.get()]);
    assert_eq!(js.h.event_loop().pending(), 2);
    assert_eq!(js.h.run_tasks(&mut js.e, 100), 2, "virtual time reaches both");
    assert_eq!((js.read(log.get(), "calls"), js.read(other.get(), "calls")), ("1".into(), "1".into()));
    let i = js.call(g.get(), "setInterval", &[f.get(), E::number(1.0)]);
    assert_eq!(js.h.run_tasks(&mut js.e, 3), 3);
    assert_eq!(js.read(log.get(), "calls"), "4");
    js.run(g.get(), "clearInterval", &[i.get()]);
    assert_eq!(js.h.event_loop().pending(), 0);
    js.run(g.get(), "queueMicrotask", &[f2.get()]);
    // the checkpoint after the call: an engine that drains its queue as
    // the outermost call returns has run it already; the stub, here
    js.e.run_microtasks(&mut js.h);
    assert_eq!(js.read(other.get(), "calls"), "2");
    let r = js.call(g.get(), "queueMicrotask", &[E::number(1.0)]);
    assert_eq!(js.threw(r), "TypeError");
    js.drop_all([i, c, b, a, f2, f, other, log, node, doc, g]);
    let Js { mut e, h } = js;
    h.close(&mut e);
}

/// A page with its window installed and virtual time, and `AbortSignal`.
fn window<E: Driver>() -> (Js<E>, madcawl_js::Owned<E::Value>) {
    let mut js = Js::<E>::new();
    js.h.install_window(&mut js.e);
    js.h.use_virtual_time();
    let g = js.e.global(js.h.realm);
    let signal = js.get(g.get(), "AbortSignal");
    js.drop(g);
    (js, signal)
}

pub fn a_timeout_signal_aborts_on_the_event_loop_and_removes_its_listeners<E: Driver>() {
    let (mut js, iface) = window::<E>();
    let s = js.call(iface.get(), "timeout", &[E::number(50.0)]);
    assert_eq!(js.read(s.get(), "aborted"), "false");
    let reason = js.get(s.get(), "reason");
    assert_eq!(js.e.kind(reason.get()), ValueKind::Undefined);
    let doc = js.doc();
    let div = js.element(doc.get(), "div");
    let (log, other) = (js.e.new_object(js.h.realm), js.e.new_object(js.h.realm));
    let f = js.e.counting(&mut js.h, log.get());
    let on = js.e.counting(&mut js.h, other.get());
    let opts = js.e.new_object(js.h.realm);
    js.set(opts.get(), "signal", s.get());
    let click = js.str("click");
    js.run(div.get(), "addEventListener", &[click.get(), f.get(), opts.get()]);
    let n = js.node(div.get());
    assert_eq!(js.h.listener_count(n), 1);
    js.set(s.get(), "onabort", on.get());
    assert!(js.is(s.get(), "onabort", on.get()));
    assert_eq!(js.h.run_tasks(&mut js.e, 100), 1, "the timer is the only task");
    assert_eq!(js.read(s.get(), "aborted"), "true");
    assert_eq!(js.h.listener_count(n), 0, "the signal removed its listener");
    assert_eq!(js.read(other.get(), "calls"), "1", "the event handler saw the abort event");
    let why = js.get(s.get(), "reason");
    assert_eq!(js.read(why.get(), "name"), "TimeoutError");
    js.run(div.get(), "addEventListener", &[click.get(), f.get(), opts.get()]);
    assert_eq!(js.h.listener_count(n), 0, "an aborted signal adds no listener");
    assert_eq!(js.h.fire(&mut js.e, n, atom!("click")), 0);
    let r = js.call(s.get(), "throwIfAborted", &[]);
    assert_eq!(js.threw(r), "TimeoutError");
    js.drop_all([why, click, opts, on, f, other, log, div, doc, reason, s, iface]);
    js.audit();
    let Js { mut e, h } = js;
    h.close(&mut e);
}

pub fn an_aborted_signal_carries_its_reason_and_its_handler_is_an_object_or_null<E: Driver>() {
    let (mut js, iface) = window::<E>();
    let plain = js.call(iface.get(), "abort", &[]);
    assert_eq!(js.read(plain.get(), "aborted"), "true");
    let why = js.get(plain.get(), "reason");
    assert_eq!((js.read(why.get(), "name"), js.read(why.get(), "code")), ("AbortError".into(), "20".into()));
    let reason = js.e.new_object(js.h.realm);
    let given = js.call(iface.get(), "abort", &[reason.get()]);
    assert!(js.is(given.get(), "reason", reason.get()));
    let none = js.call(iface.get(), "abort", &[E::null()]);
    assert_eq!(js.read(none.get(), "reason"), "null", "null is a reason");
    assert_eq!(js.read(given.get(), "onabort"), "null");
    let log = js.e.new_object(js.h.realm);
    let f = js.e.counting(&mut js.h, log.get());
    js.set(given.get(), "onabort", f.get());
    assert!(js.is(given.get(), "onabort", f.get()));
    js.set(given.get(), "onabort", E::number(5.0));
    assert_eq!(js.read(given.get(), "onabort"), "null", "a value that is not an object is null");
    for bad in [-1.0, f64::NAN, f64::INFINITY, 2f64.powi(53)] {
        let r = js.call(iface.get(), "timeout", &[E::number(bad)]);
        assert_eq!(js.threw(r), "TypeError", "timeout({bad})");
    }
    js.drop_all([f, log, none, given, reason, why, plain, iface]);
    let Js { mut e, h } = js;
    h.close(&mut e);
}

/// A timeout signal no one holds lives until its timer fires, and is
/// freed after it, with its reason and the event it fired.
pub fn a_timeout_signal_is_kept_by_its_timer_and_freed_after<E: Driver>() {
    let (mut js, iface) = window::<E>();
    super::trees::scrub();
    js.e.collect_all(&mut js.h);
    let before = js.h.side_objects();
    let log = js.e.new_object(js.h.realm);
    drop_a_timeout_signal(&mut js, iface.get(), log.get());
    super::trees::scrub();
    js.e.collect_all(&mut js.h);
    assert_eq!(js.h.side_objects(), before + 1, "the timer holds the signal");
    assert_eq!(js.h.run_tasks(&mut js.e, 10), 1);
    assert_eq!(js.read(log.get(), "calls"), "1", "its handler outlived the script's last reference");
    super::trees::scrub();
    js.e.collect_all(&mut js.h);
    assert_eq!(js.h.side_objects(), before, "the signal, its reason and its event are freed");
    js.drop_all([log, iface]);
    let Js { mut e, h } = js;
    h.close(&mut e);
}

#[inline(never)]
fn drop_a_timeout_signal<E: Driver>(js: &mut Js<E>, iface: E::Value, log: E::Value) {
    let s = js.call(iface, "timeout", &[E::number(5.0)]);
    let f = js.e.counting(&mut js.h, log);
    js.set(s.get(), "onabort", f.get());
    js.drop_all([f, s]);
}
