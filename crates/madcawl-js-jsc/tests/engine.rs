//! The adapter's own behaviour: values, script, host functions,
//! microtasks, exceptions and interrupts.

use madcawl_js::{
    Bound, ClassDef, Control, Ctx, Engine, ErrorKind, Halt, Host, HostRef, Interrupt, Key, ModuleRequest, Owned, ScriptOrigin,
};
use madcawl_js_jsc::{Jsc, Value};
use madcawl_str::DomString;

#[derive(Default)]
struct Log {
    reported: Vec<String>,
    callbacks: u32,
}

impl Host<Jsc> for Log {
    fn class_of(&self, _: HostRef) -> &'static ClassDef<Jsc, Self> {
        unreachable!("no host objects")
    }

    fn report_exception(&mut self, e: &mut Jsc, x: Owned<Value>) {
        let s = e.to_string(self, x.get()).map(|s| text(&s)).unwrap_or_default();
        self.reported.push(s);
        e.release(x);
    }

    fn load_imported_module(&mut self, _: &mut Jsc, _: &ModuleRequest<'_>, _: ()) {}

    fn interrupt_callback(&mut self, _: &mut Jsc) {
        self.callbacks += 1;
    }
}

fn text(s: &DomString) -> String {
    let mut b = Vec::new();
    s.write_wtf8(&mut b);
    String::from_utf8(b).unwrap()
}

fn eval(e: &mut Jsc, h: &mut Log, realm: madcawl_js_jsc::Realm, src: &str) -> String {
    let url = DomString::from_wtf8(b"test.js");
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    if Jsc::is_thrown(r.get()) {
        let _sentinel = r.into_raw();
        return match e.take_exception() {
            Some(x) => {
                let s = e.to_string(h, x.get()).map(|s| format!("threw {}", text(&s))).unwrap();
                e.release(x);
                s
            }
            None => "halted".to_owned(),
        };
    }
    let s = text(&e.to_string(h, r.get()).unwrap());
    e.release(r);
    s
}

/// `log(x)` appends `String(x)` to the global `seen`, as a host
/// function would.
unsafe extern "C" fn log_fn(ctx: *mut Ctx, _: u64, argv: *const u64, argc: usize) -> u64 {
    // SAFETY: bound to Jsc and Log
    let (e, h) = unsafe { Ctx::parts::<Jsc, Log>(ctx) };
    // SAFETY: the engine's arguments
    let args = unsafe { Jsc::values(argv, argc) };
    let s = e.to_string(h, args[0]).unwrap();
    h.reported.push(format!("log {}", text(&s)));
    if text(&s) == "boom" {
        return Jsc::to_bits(e.throw_error(ErrorKind::Range, "host says boom").into_raw());
    }
    Jsc::to_bits(Jsc::undefined())
}

fn setup() -> (Jsc, Log, madcawl_js_jsc::Realm) {
    let mut e = Jsc::new();
    let mut h = Log::default();
    let realm = e.new_realm();
    // SAFETY: log_fn reads its context as Jsc and Log
    let f = e.new_function(realm, "log", 1, unsafe { Bound::<_, Jsc, Log>::host_fn(log_fn) }, 0);
    let g = e.global(realm);
    assert!(e.set(&mut h, g.get(), Key::Str(&DomString::from_wtf8(b"log")), f.get()));
    e.release(g);
    e.release(f);
    (e, h, realm)
}

#[test]
fn the_entry_points_the_adapter_needs_are_exported() {
    assert_eq!(madcawl_js_jsc::missing_entry_points(), Vec::<&str>::new());
}

#[test]
fn values_and_script_round_trip() {
    let (mut e, mut h, realm) = setup();
    assert_eq!(eval(&mut e, &mut h, realm, "1 + 2"), "3");
    assert_eq!(eval(&mut e, &mut h, realm, "'h\\u00e9llo ' + '\\u4e16'"), "héllo 世");
    let s = e.string(DomString::from_wtf8("ü✓".as_bytes()));
    assert_eq!(text(&e.as_string(s.get()).unwrap()), "ü✓");
    e.release(s);
    assert_eq!(eval(&mut e, &mut h, realm, "typeof log + ' ' + log.name + ' ' + log.length"), "function log 1");
    assert_eq!(eval(&mut e, &mut h, realm, "log.call(null, 'x'); log.bind(null)('y'); 0"), "0");
    assert_eq!(h.reported, ["log x", "log y"]);
}

#[test]
fn host_exceptions_reach_script_and_script_exceptions_reach_the_host() {
    let (mut e, mut h, realm) = setup();
    let r = eval(&mut e, &mut h, realm, "try { log('boom') } catch (x) { x instanceof RangeError && x.message }");
    assert_eq!(r, "host says boom");
    assert_eq!(eval(&mut e, &mut h, realm, "null.x"), "threw TypeError: null is not an object (evaluating 'null.x')");
}

#[test]
fn microtasks_run_in_order_with_promise_jobs_and_report_their_exceptions() {
    let (mut e, mut h, realm) = setup();
    let src = "Promise.resolve().then(() => log('a')); log('sync'); 0";
    eval(&mut e, &mut h, realm, src);
    assert_eq!(h.reported, ["log sync", "log a"], "drained as the outermost call returned");
    h.reported.clear();
    // a host microtask queued with no script running waits for the checkpoint
    let g = e.global(realm);
    let f = e.get(&mut h, g.get(), Key::Str(&DomString::from_wtf8(b"log")));
    for word in ["one", "boom", "two"] {
        let arg = e.string(DomString::from_wtf8(word.as_bytes()));
        let job = e.retain(f.get());
        e.enqueue_microtask(job, arg);
    }
    assert!(h.reported.is_empty());
    e.run_microtasks(&mut h);
    assert_eq!(h.reported, ["log one", "log boom", "RangeError: host says boom", "log two"]);
    e.release(f);
    e.release(g);
}

#[test]
fn another_thread_stops_a_script_that_never_returns() {
    let (mut e, mut h, realm) = setup();
    let i = e.interrupt_handle();
    let t = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        i.terminate();
    });
    assert_eq!(eval(&mut e, &mut h, realm, "for (;;) {}"), "halted");
    t.join().unwrap();
    assert_eq!(e.halt(), Halt::Terminated);
    assert_eq!(eval(&mut e, &mut h, realm, "1"), "halted", "no script runs again");
}

#[test]
fn an_abort_ends_the_script_and_the_isolate_goes_on_after_resume() {
    let (mut e, mut h, realm) = setup();
    let i = e.interrupt_handle();
    let t = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        i.abort().unwrap();
        i.request_callback();
    });
    assert_eq!(eval(&mut e, &mut h, realm, "for (;;) {}"), "halted");
    t.join().unwrap();
    assert_eq!(e.halt(), Halt::Aborted);
    assert_eq!(e.resume(), Ok(Halt::Running));
    assert_eq!(eval(&mut e, &mut h, realm, "6 * 7"), "42");
    assert_eq!(h.callbacks, 1);
}

#[test]
fn a_window_proxy_follows_its_target() {
    let (mut e, mut h, r1) = setup();
    let r2 = e.new_realm();
    eval(&mut e, &mut h, r1, "var which = 'first'");
    eval(&mut e, &mut h, r2, "var which = 'second'");
    let p = e.new_window_proxy(r1);
    let k = DomString::from_wtf8(b"which");
    let read = |e: &mut Jsc, h: &mut Log, p: Value| {
        let v = e.get(h, p, Key::Str(&k));
        let s = text(&e.as_string(v.get()).unwrap());
        e.release(v);
        s
    };
    assert_eq!(read(&mut e, &mut h, p.get()), "first");
    e.set_window_proxy_target(p.get(), r2);
    assert_eq!(read(&mut e, &mut h, p.get()), "second");
    assert_eq!(e.window_proxy_target(p.get()), Some(r2));
    e.release(p);
}

#[test]
fn handlers_see_their_scopes_and_weak_handles_read_their_targets() {
    let (mut e, mut h, realm) = setup();
    let scope = e.new_object(realm);
    let v = e.string(DomString::from_wtf8(b"from the scope"));
    assert!(e.set(&mut h, scope.get(), Key::Str(&DomString::from_wtf8(b"near")), v.get()));
    let url = DomString::from_wtf8(b"x");
    let origin = ScriptOrigin { url: &url, line: 1, column: 0 };
    let f = e.compile_handler(&mut h, realm, &["event"], &DomString::from_wtf8(b"return near + ' ' + event"), &[scope.get()], &origin);
    assert!(!Jsc::is_thrown(f.get()));
    let r = e.call(&mut h, f.get(), Jsc::undefined(), &[Jsc::number(5.0)]);
    assert_eq!(text(&e.as_string(r.get()).unwrap()), "from the scope 5");
    let w = e.weak(scope.get());
    let got = e.weak_get(&w).expect("alive while held");
    assert_eq!(got.get(), scope.get());
    for x in [got, r, f, v, scope] {
        e.release(x);
    }
    e.weak_drop(w);
}

fn function(e: &mut Jsc, h: &mut Log, realm: madcawl_js_jsc::Realm, src: &str) -> Owned<Value> {
    let url = DomString::from_wtf8(b"test.js");
    let f = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    assert!(e.is_callable(f.get()));
    f
}

#[test]
fn a_primitive_this_reaches_the_function_as_it_is() {
    let (mut e, mut h, realm) = setup();
    let f = function(&mut e, &mut h, realm, "(function (x) { 'use strict'; return typeof this + ' ' + this + ' ' + x; })");
    let s = e.string(DomString::from_wtf8(b"s"));
    for (this, want) in [(Jsc::number(5.0), "number 5 1"), (Jsc::null(), "object null 1"), (Jsc::undefined(), "undefined undefined 1"), (s.get(), "string s 1")] {
        let r = e.call(&mut h, f.get(), this, &[Jsc::number(1.0)]);
        assert_eq!(text(&e.as_string(r.get()).unwrap()), want);
        e.release(r);
    }
    e.release(s);
    e.release(f);
}

#[test]
fn a_detached_realms_waiting_microtasks_are_dropped_unrun() {
    let (mut e, mut h, realm) = setup();
    let other = e.new_realm();
    let g = e.global(realm);
    let log = e.get(&mut h, g.get(), Key::Str(&DomString::from_wtf8(b"log")));
    e.release(g);
    let arg = e.string(DomString::from_wtf8(b"late"));
    e.enqueue_microtask(log, arg);
    e.detach_realm(realm);
    e.run_microtasks(&mut h);
    assert!(h.reported.is_empty(), "the detached realm's job ran: {:?}", h.reported);
    // the other realm's microtasks still run at a checkpoint
    let f = function(&mut e, &mut h, other, "(function (x) { globalThis.got = x; })");
    let arg = e.string(DomString::from_wtf8(b"ok"));
    e.enqueue_microtask(f, arg);
    e.run_microtasks(&mut h);
    assert_eq!(eval(&mut e, &mut h, other, "got"), "ok");
}
