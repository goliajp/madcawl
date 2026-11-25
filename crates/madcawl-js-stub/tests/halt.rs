//! Halting script: a termination ends the isolate for good and drops
//! its microtasks; an abort stops script until the host resumes it, and
//! the host chooses whether the queue survives.

mod support;

use madcawl_js::{Control, Engine, ErrorKind, Halt, Interrupt, Owned};
use madcawl_js_stub::{ClosureFn, Realm, Stub, Value};
use support::*;

/// Calls its capture after asking for the halt: the capture never runs.
fn call_after(e: &mut Stub, h: &mut Log, caps: &[Value]) {
    let r = e.call(h, caps[0], Stub::undefined(), &[]);
    assert!(Stub::is_thrown(r.into_raw()));
    assert_ne!(e.halt(), Halt::Running);
    assert!(e.take_exception().is_none(), "a halt is no exception");
}

fn terminates(e: &mut Stub, h: &mut Log, caps: &[Value], _: Value, _: &[Value]) -> Owned<Value> {
    e.interrupt_handle().terminate();
    call_after(e, h, caps);
    undefined()
}

fn aborts(e: &mut Stub, h: &mut Log, caps: &[Value], _: Value, _: &[Value]) -> Owned<Value> {
    e.interrupt_handle().abort().expect("the stub can abort");
    call_after(e, h, caps);
    undefined()
}

/// Queues a job that halts with `body`, then `left`; drains once.
fn halt_in_a_drain(e: &mut Stub, h: &mut Log, realm: Realm, log: Value, body: ClosureFn<Log>) {
    let after = counter(e, realm, log, "after");
    let job = e.new_closure::<Log>(realm, body, vec![after]);
    let left = counter(e, realm, log, "left");
    e.enqueue_microtask(job, undefined());
    e.enqueue_microtask(left, undefined());
    e.run_microtasks(h);
    assert_eq!(h.reported, 0, "a halt reports nothing");
    assert_eq!(num(e, h, log, "left"), 0.0, "the draining stopped");
    assert!(!e.in_script());
}

/// A new task: a call and a checkpoint. Whether they ran, by `task`.
fn next_task(e: &mut Stub, h: &mut Log, realm: Realm, log: Value) -> bool {
    let task = counter(e, realm, log, "task");
    let r = e.call(h, task.get(), Stub::undefined(), &[]);
    let ran = !Stub::is_thrown(r.get());
    if ran {
        e.release(r);
    } else {
        let _sentinel = r.into_raw();
    }
    e.run_microtasks(h);
    e.release(task);
    assert_eq!(num(e, h, log, "task") == 1.0, ran);
    ran
}

#[test]
fn a_termination_drops_the_queue_and_no_script_runs_again() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    halt_in_a_drain(&mut e, &mut h, realm, log.get(), terminates);
    assert_eq!(e.halt(), Halt::Terminated);
    assert_eq!(e.queued_microtasks(), 0, "what was left is dropped unrun");
    // neither a property access nor a new task brings it back
    assert_eq!(num(&mut e, &mut h, log.get(), "after"), 0.0);
    assert!(!next_task(&mut e, &mut h, realm, log.get()));
    assert_eq!(e.halt(), Halt::Terminated);
    // a job queued later is dropped as it comes
    let late = counter(&mut e, realm, log.get(), "late");
    e.enqueue_microtask(late, undefined());
    assert_eq!(e.queued_microtasks(), 0);
    e.release(log);
}

#[test]
fn a_resume_that_meets_a_termination_leaves_the_isolate_terminated() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    halt_in_a_drain(&mut e, &mut h, realm, log.get(), aborts);
    assert_eq!(e.halt(), Halt::Aborted);
    // another thread terminates just before the host resumes
    e.interrupt_handle().terminate();
    assert_eq!(e.resume(), Ok(Halt::Terminated), "the result says it did not resume");
    assert_eq!(e.halt(), Halt::Terminated);
    assert_eq!(e.queued_microtasks(), 0);
    assert!(!next_task(&mut e, &mut h, realm, log.get()));
    assert_eq!(e.resume(), Ok(Halt::Terminated), "a resume never ends a termination");
    // nothing to discard: the termination dropped the queue
    e.discard_microtasks().expect("the stub can discard");
    assert_eq!(e.queued_microtasks(), 0);
    e.release(log);
}

#[test]
fn an_abort_keeps_the_queue_and_runs_nothing_until_resumed() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    halt_in_a_drain(&mut e, &mut h, realm, log.get(), aborts);
    assert_eq!(e.halt(), Halt::Aborted);
    assert_eq!(e.queued_microtasks(), 1);
    // no later entry clears it: the next task runs nothing
    assert!(!next_task(&mut e, &mut h, realm, log.get()));
    assert_eq!(e.halt(), Halt::Aborted);
    assert_eq!(e.queued_microtasks(), 1, "the queue is kept as it was");
    assert_eq!(e.resume(), Ok(Halt::Running));
    assert!(next_task(&mut e, &mut h, realm, log.get()));
    assert_eq!(num(&mut e, &mut h, log.get(), "left"), 1.0, "the kept job ran at the checkpoint");
    assert_eq!(num(&mut e, &mut h, log.get(), "after"), 0.0);
    e.release(log);
}

#[test]
fn after_an_abort_the_host_may_drop_the_queue_before_resuming() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    halt_in_a_drain(&mut e, &mut h, realm, log.get(), aborts);
    e.discard_microtasks().expect("the stub can discard");
    assert_eq!(e.queued_microtasks(), 0);
    assert_eq!(e.resume(), Ok(Halt::Running));
    assert!(next_task(&mut e, &mut h, realm, log.get()));
    assert_eq!(num(&mut e, &mut h, log.get(), "left"), 0.0, "the dropped job never ran");
    e.release(log);
}

/// Asks for termination, then calls its capture, which therefore
/// never runs; it registers an exception before and tries to after.
fn ends_with_exceptions(e: &mut Stub, h: &mut Log, caps: &[Value], _: Value, _: &[Value]) -> Owned<Value> {
    let _sentinel = e.throw_error(ErrorKind::Type, "before").into_raw();
    e.interrupt_handle().terminate();
    let r = e.call(h, caps[0], Stub::undefined(), &[]);
    assert!(Stub::is_thrown(r.into_raw()));
    e.throw_error(ErrorKind::Type, "after")
}

#[test]
fn termination_leaves_no_exception_registered() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    let never = counter(&mut e, realm, log.get(), "never");
    let f = e.new_closure::<Log>(realm, ends_with_exceptions, vec![never]);
    let r = e.call(&mut h, f.get(), Stub::undefined(), &[]);
    assert!(Stub::is_thrown(r.into_raw()));
    assert_eq!(e.halt(), Halt::Terminated);
    assert!(e.take_exception().is_none(), "nothing for the next task to find");
    assert_eq!(num(&mut e, &mut h, log.get(), "never"), 0.0);
    e.release(f);
    e.release(log);
}

#[test]
fn an_exception_the_host_registered_before_a_terminated_call_stays() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    let f = counter(&mut e, realm, log.get(), "ran");
    let mine = e.new_object(realm);
    let kept = e.retain(mine.get());
    let _sentinel = e.throw(kept).into_raw();
    e.interrupt_handle().terminate();
    let r = e.call(&mut h, f.get(), Stub::undefined(), &[]);
    assert!(Stub::is_thrown(r.into_raw()));
    let ex = e.take_exception().expect("the host's exception");
    assert_eq!(ex.get(), mine.get());
    e.release(ex);
    // later entries make none of their own
    let r = e.call(&mut h, f.get(), Stub::undefined(), &[]);
    assert!(Stub::is_thrown(r.into_raw()));
    e.run_microtasks(&mut h);
    assert!(e.take_exception().is_none());
    assert_eq!(num(&mut e, &mut h, log.get(), "ran"), 0.0);
    for v in [mine, f, log] {
        e.release(v);
    }
}

#[test]
fn an_abort_that_comes_between_tasks_ends_no_task() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    e.interrupt_handle().abort().expect("the stub can abort");
    assert!(next_task(&mut e, &mut h, realm, log.get()), "no script ran when it came");
    assert_eq!(e.halt(), Halt::Running);
    e.release(log);
}

#[test]
fn a_job_of_a_detached_realm_is_dropped_unrun() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let (kept, gone) = (e.new_realm(), e.new_realm());
    let log = e.new_object(kept);
    let theirs = counter(&mut e, gone, log.get(), "theirs");
    let ours = counter(&mut e, kept, log.get(), "ours");
    e.enqueue_microtask(theirs, undefined());
    e.enqueue_microtask(ours, undefined());
    e.detach_realm(gone);
    e.run_microtasks(&mut h);
    assert_eq!(num(&mut e, &mut h, log.get(), "theirs"), 0.0, "no script runs in a detached realm");
    assert_eq!(num(&mut e, &mut h, log.get(), "ours"), 1.0);
    assert_eq!(e.queued_microtasks(), 0);
    e.release(log);
}

#[test]
fn a_handler_compiled_while_halted_is_thrown_with_nothing_registered() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    e.interrupt_handle().terminate();
    let (body, url) = (key("return 1"), key("about:blank"));
    let origin = madcawl_js::ScriptOrigin { url: &url, line: 1, column: 1 };
    let f = e.compile_handler(&mut h, realm, &["event"], &body, &[], &origin);
    assert!(Stub::is_thrown(f.into_raw()));
    assert_eq!(e.halt(), Halt::Terminated);
    assert!(e.take_exception().is_none(), "a halt is no exception");
}

#[test]
fn a_termination_inside_a_class_table_host_function_ends_the_call_and_the_isolate() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    let obj = Box::new(0u64);
    let w = e.new_wrapper(realm, &CLASS, madcawl_js::HostRef::new(std::ptr::from_ref(&*obj) as usize).unwrap());
    // the setter terminates, then throws: the throw is dropped
    assert!(!e.set(&mut h, w.get(), madcawl_js::Key::Str(&key("stop")), Stub::number(1.0)));
    assert_eq!(e.halt(), Halt::Terminated);
    assert!(e.take_exception().is_none(), "the setter's exception went with the termination");
    assert!(!e.in_script());
    // another member of the table runs no more
    put_nothing(&mut e, &mut h, w.get());
    assert_eq!(h.size, 0.0);
    assert!(!next_task(&mut e, &mut h, realm, log.get()));
    e.release(w);
    e.release(log);
    e.drain_host(&mut h);
}

/// Writes `size` through its setter, which does not run once halted.
fn put_nothing(e: &mut Stub, h: &mut Log, w: Value) {
    assert!(!e.set(h, w, madcawl_js::Key::Str(&key("size")), Stub::number(5.0)));
    assert!(e.take_exception().is_none());
}
