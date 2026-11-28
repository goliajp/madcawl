//! The engine interface outside the collector and halting: host
//! functions through the C calling convention, strings that are not
//! flat blocks, the microtask checkpoint, WindowProxy, and several
//! isolates on one thread.

mod support;

use madcawl_js::{Deadline, Engine, ErrorKind, Halt, HostRef, Interrupt, Control, Key, Step, ValueKind, Windows};
use madcawl_js_stub::Stub;
use madcawl_str::DomString;
use support::*;

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
    assert!(!e.in_script());
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
fn in_script_is_true_inside_any_call_and_false_outside() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    let l = e.retain(log.get());
    let inner = e.new_closure::<Log>(
        realm,
        |e, h, caps, _, _| {
            let d = e.in_script() as u8 as f64;
            put(e, h, caps[0], "inner", d);
            undefined()
        },
        vec![l],
    );
    let l = e.retain(log.get());
    let outer = e.new_closure::<Log>(
        realm,
        |e, h, caps, _, _| {
            let d = e.in_script() as u8 as f64;
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
    // only zero against nonzero is promised, not a count per function
    assert_eq!(num(&mut e, &mut h, log.get(), "inner"), 1.0);
    assert!(!e.in_script());
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
    assert_eq!(b.halt(), Halt::Running);
    let fa = counter(&mut a, ra, la.get(), "direct");
    let r = a.call(&mut ha, fa.get(), Stub::undefined(), &[]);
    assert!(Stub::is_thrown(r.into_raw()));
    assert_eq!(a.halt(), Halt::Terminated);
    assert_eq!(num(&mut b, &mut hb, lb.get(), "ran"), 1.0);
    // a goes on with its own collection and exception
    while a.collect_cycles(&mut ha, Deadline::Work(1)) == Step::More {}
    assert_eq!(a.last_collection().garbage, 2);
    let ex = a.take_exception().expect("a's own exception, registered before the termination");
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

#[test]
#[should_panic(expected = "a realm of another isolate")]
fn a_window_proxy_cannot_target_a_realm_of_another_isolate() {
    let (mut a, mut b) = (Stub::new(), Stub::new());
    let ra = a.new_realm();
    let foreign = b.new_realm();
    let proxy = a.new_window_proxy(ra);
    a.set_window_proxy_target(proxy.get(), foreign);
}

#[test]
fn a_string_value_reads_as_the_atom_that_spells_it() {
    let mut e = Stub::new();
    let realm = e.new_realm();
    let click = e.string(key("click"));
    let view = e.substring(click.get(), 0, 5);
    let held = madcawl_str::HeldAtoms::new();
    let a = e.to_atom(click.get(), &held).expect("a string");
    assert_eq!(e.to_atom(view.get(), &held), Some(a), "a view spells the same name");
    let mut b = Vec::new();
    a.write_wtf8(&mut b);
    assert_eq!(b, b"click");
    let obj = e.new_object(realm);
    assert_eq!(e.to_atom(obj.get(), &held), None);
    assert_eq!(e.to_atom(Stub::number(1.0), &held), None);
    for v in [obj, view, click] {
        e.release(v);
    }
}
