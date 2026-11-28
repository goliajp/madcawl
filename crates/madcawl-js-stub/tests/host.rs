//! What the engine hands the host: a function's data word, the
//! callbacks an interrupt request and the soft memory limit ask for,
//! the host's clock for a collection's deadline, and ToString and
//! ToNumber calling script's `toString` and `valueOf`.

mod support;

use madcawl_js::{Bound, Control, Deadline, Engine, ErrorKind, Interrupt, Key, Owned, Step, ValueKind};
use madcawl_js_stub::{Stub, Value};
use support::*;

#[test]
fn a_function_gets_its_data_word_on_every_call_and_as_a_microtask() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    // SAFETY: data_word reads its context as Stub and Log
    let f = e.new_function::<Log>(realm, "f", 0, unsafe { Bound::host_fn(data_word) }, 0xD47A);
    let r = e.call(&mut h, f.get(), Stub::undefined(), &[]);
    e.release(r);
    assert_eq!(h.data, 0xD47A);
    h.data = 0;
    let job = e.retain(f.get());
    e.enqueue_microtask(realm, job, undefined());
    e.run_microtasks(&mut h);
    assert_eq!(h.data, 0xD47A, "a host microtask carries its record in its function");
    e.release(f);
}

#[test]
fn a_callback_request_and_the_soft_limit_reach_the_host_once_each() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let log = e.new_object(realm);
    let task = counter(&mut e, realm, log.get(), "ran");
    let call = |e: &mut Stub, h: &mut Log| {
        let r = e.call(h, task.get(), Stub::undefined(), &[]);
        e.release(r);
    };
    e.interrupt_handle().request_callback();
    call(&mut e, &mut h);
    call(&mut e, &mut h);
    assert_eq!(h.callbacks, 1, "one request, one callback");
    e.set_memory_limits(1, usize::MAX);
    call(&mut e, &mut h);
    call(&mut e, &mut h);
    assert_eq!(h.pressure, 1, "told once while use stays past the limit");
    e.set_memory_limits(usize::MAX, usize::MAX);
    call(&mut e, &mut h);
    e.set_memory_limits(1, usize::MAX);
    call(&mut e, &mut h);
    assert_eq!(h.pressure, 2, "told again after use came back under it");
    assert_eq!(num(&mut e, &mut h, log.get(), "ran"), 6.0);
    e.release(task);
    e.release(log);
}

/// A ring of `n` objects that script let go.
fn ring(e: &mut Stub, h: &mut Log, realm: madcawl_js_stub::Realm, n: usize) {
    let objs: Vec<Owned<Value>> = (0..n).map(|_| e.new_object(realm)).collect();
    for i in 0..n {
        assert!(e.set(h, objs[i].get(), Key::Index(0), objs[(i + 1) % n].get()));
    }
    for o in objs {
        e.release(o);
    }
}

#[test]
fn a_time_deadline_is_read_on_the_hosts_clock() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    ring(&mut e, &mut h, realm, 200);
    h.clock = 1_000;
    assert_eq!(e.collect_cycles(&mut h, Deadline::At(500)), Step::More, "past the deadline by the host's clock");
    h.clock = 0;
    assert_eq!(e.collect_cycles(&mut h, Deadline::At(500)), Step::Done);
    assert_eq!(e.last_collection().garbage, 200);
}

/// `{ toString, valueOf }` made of the given closures, or of numbers
/// where a closure is `None`.
fn object(e: &mut Stub, h: &mut Log, realm: madcawl_js_stub::Realm, methods: [Option<madcawl_js_stub::ClosureFn<Log>>; 2]) -> Owned<Value> {
    let o = e.new_object(realm);
    for (name, m) in ["toString", "valueOf"].into_iter().zip(methods) {
        let v = match m {
            Some(body) => e.new_closure::<Log>(realm, body, vec![]),
            None => continue,
        };
        assert!(e.set(h, o.get(), Key::Str(&key(name)), v.get()));
        e.release(v);
    }
    o
}

fn text(e: &mut Stub, h: &mut Log, v: Value) -> Option<String> {
    let s = e.to_string(h, v)?;
    let mut b = Vec::new();
    s.write_wtf8(&mut b);
    Some(String::from_utf8(b).unwrap())
}

#[test]
fn tostring_and_valueof_are_called_in_the_order_each_conversion_uses() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let both = object(&mut e, &mut h, realm, [Some(|e, _, _, _, _| e.string(key("from toString"))), Some(|_, _, _, _, _| {
        // SAFETY: a number carries no count
        unsafe { Owned::from_raw(Stub::number(7.0)) }
    })]);
    assert_eq!(text(&mut e, &mut h, both.get()).as_deref(), Some("from toString"));
    assert_eq!(e.to_number(&mut h, both.get()), Some(7.0), "ToNumber asks valueOf first");
    let only_value = object(&mut e, &mut h, realm, [None, Some(|e, _, _, _, _| e.string(key("from valueOf")))]);
    assert_eq!(text(&mut e, &mut h, only_value.get()).as_deref(), Some("from valueOf"));
    let plain = e.new_object(realm);
    assert_eq!(text(&mut e, &mut h, plain.get()).as_deref(), Some("[object Object]"));
    for v in [plain, only_value, both] {
        e.release(v);
    }
}

#[test]
fn a_conversion_that_throws_or_meets_a_symbol_or_only_objects_fails() {
    let (mut e, mut h) = (Stub::new(), Log::default());
    let realm = e.new_realm();
    let throws = object(&mut e, &mut h, realm, [Some(|e, _, _, _, _| e.throw_error(ErrorKind::Range, "no")), None]);
    assert_eq!(text(&mut e, &mut h, throws.get()), None);
    assert_eq!(name_of(&mut e, &mut h), "RangeError", "the method's own exception");
    // a toString that returns an object
    let inner = e.new_object(realm);
    let f = e.new_closure::<Log>(realm, |e, _, caps, _, _| e.retain(caps[0]), vec![inner]);
    let objects = e.new_object(realm);
    assert!(e.set(&mut h, objects.get(), Key::Str(&key("toString")), f.get()));
    e.release(f);
    assert_eq!(text(&mut e, &mut h, objects.get()), None);
    assert_eq!(name_of(&mut e, &mut h), "TypeError");
    let sym = e.new_symbol(realm);
    assert_eq!(e.kind(sym.get()), ValueKind::Symbol);
    assert_eq!(text(&mut e, &mut h, sym.get()), None);
    assert_eq!(name_of(&mut e, &mut h), "TypeError");
    assert_eq!(e.to_number(&mut h, sym.get()), None);
    assert_eq!(name_of(&mut e, &mut h), "TypeError");
    assert_eq!(e.to_atom(sym.get(), &madcawl_str::HeldAtoms::new()), None);
    assert!(e.to_boolean(sym.get()));
    for v in [sym, objects, throws] {
        e.release(v);
    }
}

fn name_of(e: &mut Stub, h: &mut Log) -> String {
    let x = e.take_exception().expect("an exception");
    let n = e.get(h, x.get(), Key::Str(&key("name")));
    let s = text(e, h, n.get()).unwrap();
    e.release(n);
    e.release(x);
    s
}
