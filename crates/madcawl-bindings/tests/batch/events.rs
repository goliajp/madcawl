//! Events dispatched as script would dispatch them, with listeners that
//! are host functions, so that the stub engine runs them too: the order
//! of the phases, listeners removed and nodes moved while it runs, and
//! what an event and a listener keep alive.

use madcawl_bindings::DomHost;
use madcawl_js::{Bound, Ctx, Engine, HostFn, Key, Owned};
use madcawl_str::DomString;

use super::{Driver, Js};

/// What a listener does when called: log its label and the phase, then
/// one more thing.
pub struct Act<E: Engine> {
    label: &'static str,
    log: *mut Vec<String>,
    then: Then<E>,
}

pub enum Then<E: Engine> {
    Nothing,
    /// `on.removeEventListener('x', cb)`
    Remove { on: E::Value, cb: E::Value },
    /// The same, then `on.addEventListener('x', cb)` again.
    RemoveAndAdd { on: E::Value, cb: E::Value },
    /// `parent.removeChild(child)`
    Detach { parent: E::Value, child: E::Value },
}

fn key(name: &str) -> DomString {
    DomString::from_wtf8(name.as_bytes())
}

fn method<E: Engine>(e: &mut E, h: &mut DomHost<E>, obj: E::Value, name: &str, args: &[E::Value]) {
    let f = e.get(h, obj, Key::Str(&key(name)));
    let r = e.call(h, f.get(), obj, args);
    assert!(!E::is_thrown(r.get()), "{name} threw");
    e.release(r);
    e.release(f);
}

// SAFETY: made by `listener` for E and DomHost<E>, with an `Act` the
// test keeps alive in its data word
unsafe extern "C" fn act<E: Engine>(ctx: *mut Ctx, _: u64, argv: *const u64, argc: usize) -> u64 {
    // SAFETY: as above
    let (e, h, a, args) = unsafe {
        let a = &*((*ctx).data as *const Act<E>);
        let (e, h) = Ctx::parts::<E, DomHost<E>>(ctx);
        (e, h, a, E::values(argv, argc))
    };
    let phase = e.get(h, args[0], Key::Str(&key("eventPhase")));
    let n = e.as_number(phase.get()).expect("a number");
    e.release(phase);
    // SAFETY: the test's log, alive and not otherwise borrowed now
    unsafe { (*a.log).push(format!("{}:{n}", a.label)) };
    let x = e.string(key("x"));
    match a.then {
        Then::Nothing => {}
        Then::Remove { on, cb } => method(e, h, on, "removeEventListener", &[x.get(), cb]),
        Then::RemoveAndAdd { on, cb } => {
            method(e, h, on, "removeEventListener", &[x.get(), cb]);
            method(e, h, on, "addEventListener", &[x.get(), cb]);
        }
        Then::Detach { parent, child } => method(e, h, parent, "removeChild", &[child]),
    }
    e.release(x);
    E::to_bits(E::undefined())
}

struct Bench<E: Driver> {
    js: Js<E>,
    // boxed: the listeners write through its address while the bench moves
    #[allow(clippy::box_collection)]
    log: Box<Vec<String>>,
    acts: Vec<Box<Act<E>>>,
}

impl<E: Driver> Bench<E> {
    fn new() -> Self {
        Bench { js: Js::new(), log: Box::default(), acts: Vec::new() }
    }

    /// A listener function doing `then`.
    fn listener(&mut self, label: &'static str, then: Then<E>) -> Owned<E::Value> {
        let a = Box::new(Act { label, log: &mut *self.log, then });
        let data = std::ptr::from_ref::<Act<E>>(&a) as u64;
        self.acts.push(a);
        // SAFETY: act reads its context as E and DomHost<E>
        let f = unsafe { Bound::<HostFn, E, DomHost<E>>::host_fn(act::<E>) };
        let realm = self.js.h.realm;
        self.js.e.new_function(realm, label, 1, f, data)
    }

    /// Point a listener made earlier at what it acts on.
    fn retarget(&mut self, label: &str, then: Then<E>) {
        self.acts.iter_mut().find(|a| a.label == label).expect("a listener").then = then;
    }

    fn listen(&mut self, on: E::Value, f: E::Value, capture: bool) {
        let x = self.js.str("x");
        let c = E::boolean(capture);
        self.js.run(on, "addEventListener", &[x.get(), f, c]);
        self.js.drop(x);
    }

    fn dispatch(&mut self, at: E::Value, bubbles: bool) -> Owned<E::Value> {
        let doc = self.js.doc();
        let kind = self.js.str("Event");
        let ev = self.js.call(doc.get(), "createEvent", &[kind.get()]);
        let x = self.js.str("x");
        self.js.run(ev.get(), "initEvent", &[x.get(), E::boolean(bubbles), E::boolean(true)]);
        self.js.run(at, "dispatchEvent", &[ev.get()]);
        self.js.drop_all([x, kind, doc]);
        ev
    }

    fn take_log(&mut self) -> String {
        std::mem::take(&mut *self.log).join(",")
    }

    fn close(self) {
        let Bench { js, .. } = self;
        let Js { mut e, h } = js;
        h.close(&mut e);
    }
}

/// `html > div > span` in the document.
fn tree<E: Driver>(js: &mut Js<E>) -> [Owned<E::Value>; 3] {
    let doc = js.doc();
    let html = js.element(doc.get(), "html");
    let div = js.element(doc.get(), "div");
    let span = js.element(doc.get(), "span");
    js.run(doc.get(), "appendChild", &[html.get()]);
    js.run(html.get(), "appendChild", &[div.get()]);
    js.run(div.get(), "appendChild", &[span.get()]);
    js.drop(doc);
    [html, div, span]
}

pub fn dispatch_runs_capture_target_and_bubble_listeners_in_order<E: Driver>() {
    let mut b = Bench::<E>::new();
    let [html, div, span] = tree(&mut b.js);
    let fs = [
        (b.listener("html-c", Then::Nothing), html.get(), true),
        (b.listener("div-b", Then::Nothing), div.get(), false),
        (b.listener("span-b", Then::Nothing), span.get(), false),
        (b.listener("span-c", Then::Nothing), span.get(), true),
        (b.listener("div-c", Then::Nothing), div.get(), true),
    ];
    for (f, on, c) in &fs {
        b.listen(*on, f.get(), *c);
    }
    let ev = b.dispatch(span.get(), true);
    assert_eq!(b.take_log(), "html-c:1,div-c:1,span-c:2,span-b:2,div-b:3");
    b.js.drop(ev);
    let ev = b.dispatch(span.get(), false);
    assert_eq!(b.take_log(), "html-c:1,div-c:1,span-c:2,span-b:2", "no bubbling");
    b.js.drop(ev);
    for (f, _, _) in fs {
        b.js.drop(f);
    }
    b.js.drop_all([span, div, html]);
    b.js.audit();
    b.close();
}

pub fn a_listener_removed_during_dispatch_is_not_called_even_when_added_again<E: Driver>() {
    let mut b = Bench::<E>::new();
    let [html, div, span] = tree(&mut b.js);
    let first = b.listener("first", Then::Nothing);
    let second = b.listener("second", Then::Nothing);
    b.retarget("first", Then::Remove { on: span.get(), cb: second.get() });
    b.listen(span.get(), first.get(), false);
    b.listen(span.get(), second.get(), false);
    let ev = b.dispatch(span.get(), false);
    assert_eq!(b.take_log(), "first:2", "the later listener was removed");
    b.js.drop(ev);
    let ev = b.dispatch(span.get(), false);
    assert_eq!(b.take_log(), "first:2");
    b.js.drop(ev);
    // removed and added again: a new listener, not called by this dispatch
    b.retarget("first", Then::RemoveAndAdd { on: span.get(), cb: second.get() });
    b.listen(span.get(), second.get(), false);
    let ev = b.dispatch(span.get(), false);
    assert_eq!(b.take_log(), "first:2");
    b.js.drop(ev);
    b.retarget("first", Then::Nothing);
    let ev = b.dispatch(span.get(), false);
    assert_eq!(b.take_log(), "first:2,second:2", "the one added again runs next time");
    b.js.drop(ev);
    b.js.drop_all([first, second, span, div, html]);
    b.js.audit();
    b.close();
}

pub fn removing_nodes_during_dispatch_leaves_its_path_as_it_was<E: Driver>() {
    let mut b = Bench::<E>::new();
    let [html, div, span] = tree(&mut b.js);
    let cut = b.listener("cut", Then::Detach { parent: html.get(), child: div.get() });
    let at = b.listener("span", Then::Nothing);
    let up = b.listener("div", Then::Nothing);
    let top = b.listener("html", Then::Nothing);
    b.listen(div.get(), cut.get(), true);
    b.listen(span.get(), at.get(), false);
    b.listen(div.get(), up.get(), false);
    b.listen(html.get(), top.get(), false);
    let ev = b.dispatch(span.get(), true);
    assert_eq!(b.take_log(), "cut:1,span:2,div:3,html:3", "the path was fixed when the dispatch began");
    assert!(b.js.is(ev.get(), "target", span.get()));
    let parent = b.js.get(div.get(), "parentNode");
    assert!(parent.get() == E::null(), "the div left the document");
    b.js.drop(parent);
    b.js.drop(ev);
    b.js.audit();
    b.js.drop_all([cut, at, up, top, span, div, html]);
    b.js.e.collect_all(&mut b.js.h);
    b.js.audit();
    b.close();
}

pub fn an_event_keeps_its_detached_target_wrapper_with_its_own_properties<E: Driver>() {
    let mut b = Bench::<E>::new();
    let mut held = Vec::new();
    dispatch_at_a_dropped_tree(&mut b, &mut held);
    super::trees::scrub();
    b.js.e.collect_all(&mut b.js.h);
    let ev = held.pop().expect("the event");
    let target = b.js.get(ev.get(), "target");
    assert_eq!(b.js.read(target.get(), "x"), "kept");
    let child = b.js.get(target.get(), "firstChild");
    assert_eq!(b.js.read(child.get(), "y"), "too");
    b.js.drop_all([child, target, ev]);
    b.js.audit();
    b.close();
}

/// A div with a child, each with a property of its own, an event
/// dispatched at the div, and every wrapper of the tree let go of.
#[inline(never)]
fn dispatch_at_a_dropped_tree<E: Driver>(b: &mut Bench<E>, held: &mut Vec<Owned<E::Value>>) {
    let doc = b.js.doc();
    let div = b.js.element(doc.get(), "div");
    let i = b.js.element(doc.get(), "i");
    b.js.run(div.get(), "appendChild", &[i.get()]);
    let (kept, too) = (b.js.str("kept"), b.js.str("too"));
    b.js.set(div.get(), "x", kept.get());
    b.js.set(i.get(), "y", too.get());
    let ev = b.dispatch(div.get(), false);
    b.js.drop_all([kept, too, i, div, doc]);
    held.push(ev);
}

pub fn a_listener_object_holding_its_own_detached_node_is_freed<E: Driver>() {
    let mut js = Js::<E>::new();
    let before = js.h.dom.node_count();
    for _ in 0..50 {
        listen_on_a_dropped_node(&mut js);
    }
    super::trees::scrub();
    js.e.collect_all(&mut js.h);
    assert_eq!(js.h.dom.node_count(), before);
    js.audit();
    let Js { mut e, h } = js;
    h.close(&mut e);
}

/// A div whose listener is an object holding the div, both let go of.
#[inline(never)]
fn listen_on_a_dropped_node<E: Driver>(js: &mut Js<E>) {
    let doc = js.doc();
    let div = js.element(doc.get(), "div");
    let obj = js.e.new_object(js.h.realm);
    js.set(obj.get(), "node", div.get());
    let log = js.e.new_object(js.h.realm);
    let f = js.e.counting(&mut js.h, log.get());
    js.set(obj.get(), "handleEvent", f.get());
    let x = js.str("x");
    js.run(div.get(), "addEventListener", &[x.get(), obj.get()]);
    js.drop_all([x, f, log, obj, div, doc]);
}
