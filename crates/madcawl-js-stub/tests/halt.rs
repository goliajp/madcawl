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
#[should_panic(expected = "a terminated isolate resumed")]
fn a_terminated_isolate_is_never_resumed() {
    let mut e = Stub::new();
    e.interrupt_handle().terminate();
    let _ = e.resume();
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
    e.resume().expect("the stub can resume");
    assert_eq!(e.halt(), Halt::Running);
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
    e.resume().expect("the stub can resume");
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
