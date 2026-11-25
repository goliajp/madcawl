//! Script halted inside a listener and inside a class table's host
//! function. A dispatch that meets a halt calls no further listener,
//! reports nothing and runs no checkpoint; a terminated isolate runs no
//! script again, and an aborted one goes on with the next task once the
//! host resumes it.

mod support;

use madcawl_bindings::DomHost;
use madcawl_js::{Control, Engine, Halt, Interrupt};
use madcawl_js_stub::{Stub, Value};
use madcawl_str::atom;
use support::{Js, V, call_in};

/// A listener that queues job `caps[0]`, then terminates the isolate.
fn terminates(e: &mut Stub, _: &mut DomHost<Stub>, caps: &[Value], _: Value, _: &[Value]) -> V {
    let job = e.retain(caps[0]);
    e.enqueue_microtask(job, support::undefined());
    e.interrupt_handle().terminate();
    support::undefined()
}

/// A listener that queues job `caps[0]`, then aborts the running script
/// when `caps[1]`'s `armed` is truthy.
fn aborts(e: &mut Stub, h: &mut DomHost<Stub>, caps: &[Value], _: Value, _: &[Value]) -> V {
    let armed = e.get(h, caps[1], madcawl_js::Key::Str(&support::key("armed")));
    let go = e.to_boolean(armed.get());
    e.release(armed);
    if go {
        let job = e.retain(caps[0]);
        e.enqueue_microtask(job, support::undefined());
        e.interrupt_handle().abort().expect("the stub can abort");
    }
    support::undefined()
}

/// A div with a listener of body `first` over `caps`, then a counting
/// listener on `log`; both for click.
fn two_listeners(js: &mut Js, first: madcawl_js_stub::ClosureFn<DomHost<Stub>>, caps: Vec<V>, log: Value) -> V {
    let div = js.element("div");
    let click = js.str("click");
    let f1 = js.function(first, caps);
    let l = js.retain(log);
    let f2 = js.closure(vec![l]);
    js.run(div.get(), "addEventListener", &[click.get(), f1.get()]);
    js.run(div.get(), "addEventListener", &[click.get(), f2.get()]);
    for v in [f2, f1, click] {
        js.drop(v);
    }
    div
}

#[test]
fn a_termination_in_a_listener_ends_the_dispatch_and_the_isolate() {
    let mut js = Js::new();
    let log = js.object();
    let l = js.retain(log.get());
    let job = js.closure(vec![l]);
    let j = js.retain(job.get());
    let div = two_listeners(&mut js, terminates, vec![j], log.get());
    let n = js.node(div.get());
    assert_eq!(js.h.fire(&mut js.e, n, atom!("click")), 1, "the second listener is not called");
    assert_eq!(js.e.halt(), Halt::Terminated);
    assert_eq!(js.h.reported, 0, "a termination is not reported");
    assert!(js.e.take_exception().is_none());
    assert_eq!(js.e.queued_microtasks(), 0, "the job it queued is dropped, not run at a checkpoint");
    assert_eq!(js.number(log.get(), "calls"), 0.0, "neither the job nor the second listener ran");
    // the next task runs nothing
    assert_eq!(js.h.fire(&mut js.e, n, atom!("click")), 1);
    assert_eq!(js.number(log.get(), "calls"), 0.0);
    for v in [div, job, log] {
        js.drop(v);
    }
    let Js { mut e, h } = js;
    h.close(&mut e);
    assert_eq!(e.live_cells(), 0, "no cell outlives its host after a termination");
}

#[test]
fn an_abort_in_a_listener_ends_the_dispatch_and_the_next_task_runs_after_resume() {
    let mut js = Js::new();
    let log = js.object();
    let l = js.retain(log.get());
    let job = js.function(
        |e, h, caps, _, _| {
            assert!(e.set(h, caps[0], madcawl_js::Key::Str(&support::key("job")), Stub::number(1.0)));
            support::undefined()
        },
        vec![l],
    );
    let state = js.object();
    js.set(state.get(), "armed", Stub::boolean(true));
    let (j, s) = (js.retain(job.get()), js.retain(state.get()));
    let div = two_listeners(&mut js, aborts, vec![j, s], log.get());
    let n = js.node(div.get());
    assert_eq!(js.h.fire(&mut js.e, n, atom!("click")), 1);
    assert_eq!(js.e.halt(), Halt::Aborted);
    assert_eq!(js.h.reported, 0);
    assert_eq!(js.e.queued_microtasks(), 1, "an abort keeps the queue");
    assert_eq!(js.number(log.get(), "job"), 0.0, "no checkpoint after the halted listener");
    assert_eq!(js.e.resume(), Ok(Halt::Running));
    js.set(state.get(), "armed", Stub::boolean(false));
    assert_eq!(js.h.fire(&mut js.e, n, atom!("click")), 2, "the same isolate runs the next task");
    assert_eq!(js.number(log.get(), "calls"), 1.0);
    assert_eq!(js.number(log.get(), "job"), 1.0, "the kept job ran at that task's checkpoint");
    for v in [div, state, job, log] {
        js.drop(v);
    }
}

/// `toString` that terminates the isolate, then returns a string.
fn ends(e: &mut Stub, _: &mut DomHost<Stub>, _: &[Value], _: Value, _: &[Value]) -> V {
    e.interrupt_handle().terminate();
    e.string(support::key("click"))
}

#[test]
fn a_termination_inside_a_class_table_host_function_leaves_no_exception_and_no_effect() {
    let mut js = Js::new();
    let div = js.element("div");
    let f = js.closure(vec![]);
    let to_string = js.function(ends, vec![]);
    let ty = js.object();
    js.set(ty.get(), "toString", to_string.get());
    let before = js.h.dom.node_count();
    // addEventListener's conversion of its type runs the toString
    let r = js.call(div.get(), "addEventListener", &[ty.get(), f.get()]);
    assert!(Stub::is_thrown(r.into_raw()));
    assert_eq!(js.e.halt(), Halt::Terminated);
    assert!(js.e.take_exception().is_none(), "nothing for the host to report");
    assert_eq!(js.h.listener_count(js.node(div.get())), 0, "the member did not act");
    assert!(!js.e.in_script());
    // no later call into a class table runs
    let doc = js.doc();
    let name = js.str("p");
    let threw = call_in(&mut js.e, &mut js.h, doc.get(), "createElement", &[name.get()]);
    assert!(threw);
    assert!(js.e.take_exception().is_none());
    assert_eq!(js.h.dom.node_count(), before, "no element made");
    for v in [name, doc, ty, to_string, f, div] {
        js.drop(v);
    }
}
