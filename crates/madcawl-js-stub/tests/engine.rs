//! The engine interface outside the collector: host functions through
//! the C calling convention, strings that are not flat blocks, the
//! microtask checkpoint and termination, WindowProxy, and several
//! isolates on one thread.

use madcawl_js::{
    Attribute, Bound, ClassDef, ClassId, Control, Ctx, Deadline, Engine, ErrorKind, Host, HostOps,
    HostRef, Interceptors, Interrupt, Key, Owned, Step, ValueKind,
};
use madcawl_js_stub::{Stub, Value};
use madcawl_str::DomString;

/// A host that counts what it is told, with one host object class.
#[derive(Default)]
struct Log {
    reported: u32,
    size: f64,
}

static OPS: HostOps<Stub, Log> = HostOps {
    traverse: |_, _, _| {},
    unlink: |_, _, _| {},
    can_skip: |_, _| true,
    wrapper_needs_preserve: |_, _| {},
    wrapper_died: |_, _| {},
};

// SAFETY: these functions read their context as Stub and Log
static INTERCEPTORS: Interceptors<Stub, Log> = Interceptors { index: Some(unsafe { Bound::index(item) }), length: None };

static CLASS: ClassDef<Stub, Log> = ClassDef {
    name: "List",
    id: ClassId(1),
    last: ClassId(1),
    parent: None,
    // SAFETY: as for INTERCEPTORS
    attributes: &[Attribute { name: "size", get: unsafe { Bound::host_fn(size) }, set: Some(unsafe { Bound::host_fn(set_size) }) }],
    operations: &[],
    constants: &[],
    interceptors: Some(&INTERCEPTORS),
    ops: Some(&OPS),
};

impl Host<Stub> for Log {
    fn class_of(&self, _: HostRef) -> &'static ClassDef<Stub, Log> {
        &CLASS
    }

    fn report_exception(&mut self, e: &mut Stub, exception: Owned<Value>) {
        self.reported += 1;
        e.release(exception);
    }

    fn load_imported_module(&mut self, _: &mut Stub, _: &madcawl_js::ModuleRequest<'_>, _: ()) {}
}

/// Two items, 10 and 11.
unsafe extern "C" fn item(ctx: *mut Ctx, _this: u64, index: u32) -> u64 {
    // SAFETY: called with a context for Log
    let (e, _) = unsafe { Ctx::parts::<Stub, Log>(ctx) };
    assert_eq!(e.script_depth(), 1);
    match index {
        0 | 1 => Stub::to_bits(Stub::number(10.0 + index as f64)),
        _ => Stub::to_bits(Stub::absent()),
    }
}

unsafe extern "C" fn size(ctx: *mut Ctx, _this: u64, _argv: *const u64, argc: usize) -> u64 {
    assert_eq!(argc, 0, "a getter gets no arguments");
    // SAFETY: called with a context for Log
    let (_, h) = unsafe { Ctx::parts::<Stub, Log>(ctx) };
    Stub::to_bits(Stub::number(h.size))
}

unsafe extern "C" fn set_size(ctx: *mut Ctx, _this: u64, argv: *const u64, argc: usize) -> u64 {
    // SAFETY: called with a context for Log and its one argument
    let ((e, h), args) = unsafe { (Ctx::parts::<Stub, Log>(ctx), Stub::values(argv, argc)) };
    h.size = e.as_number(args[0]).unwrap();
    Stub::to_bits(Stub::undefined())
}

fn key(name: &str) -> DomString {
    DomString::from_wtf8(name.as_bytes())
}

fn num(e: &mut Stub, h: &mut Log, obj: Value, name: &str) -> f64 {
    let v = e.get(h, obj, Key::Str(&key(name)));
    let n = e.as_number(v.get()).unwrap_or(0.0);
    e.release(v);
    n
}

fn put(e: &mut Stub, h: &mut Log, obj: Value, name: &str, n: f64) {
    assert!(e.set(h, obj, Key::Str(&key(name)), Stub::number(n)));
}

fn bump(e: &mut Stub, h: &mut Log, obj: Value, name: &str) {
    let n = num(e, h, obj, name);
    put(e, h, obj, name, n + 1.0);
}

fn undefined() -> Owned<Value> {
    // SAFETY: undefined carries no count
    unsafe { Owned::from_raw(Stub::undefined()) }
}

/// A job that counts its runs in `name` on the object it closes over.
fn counter(e: &mut Stub, realm: madcawl_js_stub::Realm, log: Value, name: &'static str) -> Owned<Value> {
    let log = e.retain(log);
    let tag = e.string(key(name));
    e.new_closure::<Log>(
        realm,
        |e, h, caps, _, _| {
            let name = e.as_string(caps[1]).unwrap();
            bump(e, h, caps[0], name.as_ascii_str().unwrap());
            undefined()
        },
        vec![log, tag],
    )
}

#[test]
fn class_members_are_called_through_the_c_convention_and_an_absent_index_reads_undefined() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let obj = Box::new(0u64);
    let w = e.new_wrapper(realm, &CLASS, HostRef::new(std::ptr::from_ref(&*obj) as usize).unwrap());
    for (i, want) in [(0, Some(10.0)), (1, Some(11.0)), (2, None)] {
        let v = e.get(&mut h, w.get(), Key::Index(i));
        assert_eq!(e.as_number(v.get()), want);
        if want.is_none() {
            assert_eq!(v.get(), Stub::undefined(), "the sentinel never reaches script");
        }
        e.release(v);
    }
    put(&mut e, &mut h, w.get(), "size", 3.0);
    assert_eq!(h.size, 3.0);
    assert_eq!(num(&mut e, &mut h, w.get(), "size"), 3.0);
    assert_eq!(e.script_depth(), 0);
    e.release(w);
    e.drain_host(&mut h);
}

#[test]
fn a_string_that_is_a_view_is_flattened_into_a_new_block() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let whole = e.string(key("xhello"));
    let view = e.substring(whole.get(), 1, 5);
    let empty = e.substring(whole.get(), 0, 0);
    assert_eq!(e.kind(view.get()), ValueKind::String);
    let flat = e.as_string(view.get()).unwrap();
    assert_eq!(flat, key("hello"));
    assert_eq!(e.to_string(&mut h, view.get()).unwrap(), key("hello"));
    assert!(e.to_boolean(view.get()));
    assert!(!e.to_boolean(empty.get()));
    let same = e.as_string(whole.get()).unwrap().into_raw();
    assert_eq!(same.as_ptr() as u64, Stub::to_bits(whole.get()), "a flat string is its own block");
    // SAFETY: the count as_string gave
    drop(unsafe { DomString::from_raw(same) });
    for v in [empty, view, whole] {
        e.release(v);
    }
}

#[test]
fn the_script_depth_counts_script_and_host_functions_on_the_stack() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    let l = e.retain(log.get());
    let inner = e.new_closure::<Log>(
        realm,
        |e, h, caps, _, _| {
            let d = e.script_depth() as f64;
            put(e, h, caps[0], "inner", d);
            undefined()
        },
        vec![l],
    );
    let l = e.retain(log.get());
    let outer = e.new_closure::<Log>(
        realm,
        |e, h, caps, _, _| {
            let d = e.script_depth() as f64;
            put(e, h, caps[0], "outer", d);
            let r = e.call(h, caps[1], Stub::undefined(), &[]);
            e.release(r);
            undefined()
        },
        vec![l, inner],
    );
    let r = e.call(&mut h, outer.get(), Stub::undefined(), &[]);
    e.release(r);
    assert_eq!(num(&mut e, &mut h, log.get(), "outer"), 1.0);
    assert_eq!(num(&mut e, &mut h, log.get(), "inner"), 2.0);
    assert_eq!(e.script_depth(), 0);
    e.release(outer);
    e.release(log);
}

#[test]
fn a_checkpoint_follows_the_four_rules() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    let later = counter(&mut e, realm, log.get(), "later");
    let l = e.retain(log.get());
    // queues `later`, then enters a checkpoint while in one
    let first = e.new_closure::<Log>(
        realm,
        |e, h, caps, _, _| {
            let job = e.retain(caps[1]);
            e.enqueue_microtask(job, undefined());
            e.run_microtasks(h);
            let seen = num(e, h, caps[0], "later");
            put(e, h, caps[0], "later_seen_inside", seen);
            undefined()
        },
        vec![l, later],
    );
    let failing = e.new_closure::<Log>(realm, |e, _, _, _, _| e.throw_error(ErrorKind::Type, "fails"), vec![]);
    e.enqueue_microtask(first, undefined());
    e.enqueue_microtask(failing, undefined());
    e.run_microtasks(&mut h);
    assert_eq!(num(&mut e, &mut h, log.get(), "later_seen_inside"), 0.0, "a nested checkpoint returns at once");
    assert_eq!(num(&mut e, &mut h, log.get(), "later"), 1.0, "queued while draining, run in the same drain");
    assert_eq!(h.reported, 1, "the exception was reported and the draining went on");
    assert!(e.take_exception().is_none());
    e.release(log);
}

#[test]
fn termination_stops_the_drain_reports_nothing_and_the_next_task_runs() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    let after = counter(&mut e, realm, log.get(), "after");
    let l = e.retain(log.get());
    // asks for termination, then calls a function that therefore never runs
    let ending = e.new_closure::<Log>(
        realm,
        |e, h, caps, _, _| {
            e.interrupt_handle().terminate();
            let r = e.call(h, caps[1], Stub::undefined(), &[]);
            assert!(Stub::is_thrown(r.into_raw()));
            assert!(e.is_terminating());
            assert!(e.take_exception().is_none(), "termination is no exception");
            undefined()
        },
        vec![l, after],
    );
    let left = counter(&mut e, realm, log.get(), "left");
    let next = counter(&mut e, realm, log.get(), "next");
    e.enqueue_microtask(ending, undefined());
    e.enqueue_microtask(left, undefined());
    e.run_microtasks(&mut h);
    assert!(e.is_terminating());
    assert_eq!(h.reported, 0);
    assert_eq!(num(&mut e, &mut h, log.get(), "left"), 0.0, "the draining stopped");
    assert_eq!(e.script_depth(), 0);
    // the next task calls into script afresh
    let task = counter(&mut e, realm, log.get(), "task");
    let r = e.call(&mut h, task.get(), Stub::undefined(), &[]);
    assert!(!Stub::is_thrown(r.get()));
    e.release(r);
    assert!(!e.is_terminating());
    e.enqueue_microtask(next, undefined());
    e.run_microtasks(&mut h);
    // what was left in the queue runs at the next checkpoint
    for (name, want) in [("after", 0.0), ("left", 1.0), ("task", 1.0), ("next", 1.0)] {
        assert_eq!(num(&mut e, &mut h, log.get(), name), want, "{name}");
    }
    e.release(task);
    e.release(log);
}

#[test]
fn a_window_proxy_keeps_its_identity_across_navigation() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let first = e.new_realm();
    let proxy = e.new_window_proxy(first);
    assert_eq!(e.window_proxy_target(proxy.get()), Some(first));
    put(&mut e, &mut h, proxy.get(), "x", 1.0);
    let g1 = e.global(first);
    assert_eq!(num(&mut e, &mut h, g1.get(), "x"), 1.0, "a write lands on the global");
    let second = e.new_realm();
    e.set_window_proxy_target(proxy.get(), second);
    e.detach_realm(first);
    assert_eq!(e.window_proxy_target(proxy.get()), Some(second));
    let x = e.get(&mut h, proxy.get(), Key::Str(&key("x")));
    assert_eq!(x.get(), Stub::undefined(), "the new global has no x");
    put(&mut e, &mut h, proxy.get(), "y", 2.0);
    let g2 = e.global(second);
    assert_eq!(num(&mut e, &mut h, g2.get(), "y"), 2.0);
    assert_eq!(num(&mut e, &mut h, g1.get(), "y"), 0.0);
    let plain = e.new_object(second);
    assert_eq!(e.window_proxy_target(plain.get()), None);
    for v in [plain, g2, g1, proxy] {
        e.release(v);
    }
    e.detach_realm(second);
    assert_eq!(e.live_cells(), 0);
}

#[test]
fn two_isolates_on_one_thread_keep_their_own_state() {
    let (mut a, mut ha) = (Stub::new(), Log::default());
    let (mut b, mut hb) = (Stub::new(), Log::default());
    let (ra, rb) = (a.new_realm(), b.new_realm());
    let (la, lb) = (a.new_object(ra), b.new_object(rb));
    // a cycle candidate in a only
    let x = a.new_object(ra);
    let y = a.new_object(ra);
    assert!(a.set(&mut ha, x.get(), Key::Index(0), y.get()));
    assert!(a.set(&mut ha, y.get(), Key::Index(0), x.get()));
    a.release(x);
    a.release(y);
    // a's collection stops after its first batch; b runs meanwhile
    assert_eq!(a.collect_cycles(&mut ha, Deadline::Work(0)), Step::More);
    assert_eq!(b.collect_cycles(&mut hb, Deadline::Work(100)), Step::Done);
    assert_eq!(b.last_collection().visited, 0, "a's candidates are not b's");
    // a pending exception and a queued microtask in a
    let t = a.throw_error(ErrorKind::Type, "a's");
    let _sentinel = t.into_raw();
    let job = counter(&mut a, ra, la.get(), "ran");
    a.enqueue_microtask(job, undefined());
    assert!(b.take_exception().is_none());
    b.run_microtasks(&mut hb);
    assert_eq!(num(&mut a, &mut ha, la.get(), "ran"), 0.0, "b's checkpoint runs only b's queue");
    // a is terminated; b runs on
    a.interrupt_handle().terminate();
    let fb = counter(&mut b, rb, lb.get(), "ran");
    let r = b.call(&mut hb, fb.get(), Stub::undefined(), &[]);
    assert!(!Stub::is_thrown(r.into_raw()));
    assert!(!b.is_terminating());
    let fa = counter(&mut a, ra, la.get(), "direct");
    let r = a.call(&mut ha, fa.get(), Stub::undefined(), &[]);
    assert!(Stub::is_thrown(r.into_raw()));
    assert!(a.is_terminating());
    assert_eq!(num(&mut b, &mut hb, lb.get(), "ran"), 1.0);
    // a goes on with its own collection and exception
    while a.collect_cycles(&mut ha, Deadline::Work(1)) == Step::More {}
    assert_eq!(a.last_collection().garbage, 2);
    let ex = a.take_exception().expect("a's own exception");
    a.release(ex);
    for (e, vs) in [(&mut a, vec![fa, la]), (&mut b, vec![fb, lb])] {
        for v in vs {
            e.release(v);
        }
    }
}

#[test]
#[should_panic(expected = "a value of another isolate")]
fn a_value_of_one_isolate_is_refused_by_another() {
    let (mut a, mut ha) = (Stub::new(), Log::default());
    let mut b = Stub::new();
    let rb = b.new_realm();
    let foreign = b.new_object(rb);
    let _ = a.get(&mut ha, foreign.get(), Key::Str(&key("x")));
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
    assert!(e.is_terminating());
    assert!(e.take_exception().is_none(), "nothing for the next task to find");
    assert_eq!(num(&mut e, &mut h, log.get(), "never"), 0.0);
    e.release(f);
    e.release(log);
}

#[test]
fn a_next_task_that_is_only_a_checkpoint_runs_its_microtasks() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    let never = counter(&mut e, realm, log.get(), "never");
    let f = e.new_closure::<Log>(realm, ends_with_exceptions, vec![never]);
    let job = counter(&mut e, realm, log.get(), "job");
    let r = e.call(&mut h, f.get(), Stub::undefined(), &[]);
    assert!(Stub::is_thrown(r.into_raw()));
    assert!(e.is_terminating());
    // the host skips this task's checkpoint; the next task queues a job
    // and only runs a checkpoint
    e.enqueue_microtask(job, undefined());
    e.run_microtasks(&mut h);
    assert!(!e.is_terminating());
    assert_eq!(num(&mut e, &mut h, log.get(), "job"), 1.0);
    e.release(f);
    e.release(log);
}

#[test]
#[should_panic(expected = "a detached realm")]
fn a_detached_realm_gives_no_global() {
    let mut e = Stub::new();
    let realm = e.new_realm();
    e.detach_realm(realm);
    let _ = e.global(realm);
}

#[test]
#[should_panic(expected = "a detached realm")]
fn a_window_proxy_cannot_target_a_detached_realm() {
    let mut e = Stub::new();
    let (first, second) = (e.new_realm(), e.new_realm());
    let proxy = e.new_window_proxy(first);
    e.detach_realm(second);
    e.set_window_proxy_target(proxy.get(), second);
}
