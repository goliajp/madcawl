//! What a page or a closed realm could once turn into a panic or a
//! leak in the adapter: no live realm left, a page that replaced the
//! built-ins the adapter's helpers use, a stop asked for while a
//! WindowProxy is made.

use madcawl_js::{ClassDef, Control, Engine, ErrorKind, Host, HostRef, Interrupt, Key, ModuleRequest, Owned, ScriptOrigin, Windows};
use madcawl_js_jsc::{Jsc, Realm, Value};
use madcawl_str::DomString;

#[derive(Default)]
struct Quiet {
    reported: u32,
}

impl Host<Jsc> for Quiet {
    fn class_of(&self, _: HostRef) -> &'static ClassDef<Jsc, Self> {
        unreachable!("no host objects")
    }

    fn report_exception(&mut self, e: &mut Jsc, x: Owned<Value>) {
        self.reported += 1;
        e.release(x);
    }

    fn load_imported_module(&mut self, _: &mut Jsc, _: &ModuleRequest<'_>, _: ()) {}
}

fn run(e: &mut Jsc, h: &mut Quiet, realm: Realm, src: &str) -> Owned<Value> {
    let url = DomString::from_wtf8(b"followups.js");
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    assert!(!Jsc::is_thrown(r.get()), "{src} threw");
    r
}

fn number(e: &mut Jsc, h: &mut Quiet, realm: Realm, src: &str) -> f64 {
    let r = run(e, h, realm, src);
    let n = e.as_number(r.get()).expect("a number");
    e.release(r);
    n
}

#[test]
fn with_every_realm_detached_weak_handles_errors_and_calls_still_work() {
    let (mut e, mut h) = (Jsc::new(), Quiet::default());
    let realm = e.new_realm();
    let obj = run(&mut e, &mut h, realm, "({ n: 1 })");
    let f = run(&mut e, &mut h, realm, "(function () { 'use strict'; return typeof this; })");
    let held = e.weak(obj.get());
    e.detach_realm(realm);
    assert_eq!(e.live_realms(), 0);
    let w = e.weak(obj.get());
    for handle in [&held, &w] {
        let got = e.weak_get(handle).expect("alive while held");
        assert_eq!(got.get(), obj.get());
        e.release(got);
    }
    let thrown = e.throw_error(ErrorKind::Type, "no realm");
    assert!(Jsc::is_thrown(thrown.into_raw()));
    let x = e.take_exception().expect("an error registered");
    e.release(x);
    let r = e.call(&mut h, f.get(), Jsc::number(1.0), &[]);
    assert!(Jsc::is_thrown(r.into_raw()), "a call with a primitive this needs a live realm");
    let x = e.take_exception().expect("a TypeError registered");
    e.release(x);
    e.weak_drop(held);
    e.weak_drop(w);
    e.release(f);
    e.release(obj);
}

/// A page that replaces what the adapter's helpers would otherwise look
/// up as it runs.
const HOSTILE: &str = "
  const boom = () => { throw new Error('page code ran'); };
  Promise.prototype.then = boom;
  Array.prototype[Symbol.iterator] = boom;
  Array.prototype.map = boom;
  Array.prototype.push = boom;
  Set.prototype.add = boom;
  WeakMap.prototype.get = boom;
  WeakRef.prototype.deref = boom;
  Reflect.get = boom;
  globalThis.Proxy = boom;
  globalThis.TypeError = boom;
  Object.defineProperty(Object.prototype, 'get', { get: boom, configurable: true });
  Object.defineProperty(Object.prototype, 'value', { get: boom, configurable: true });
  0";

#[test]
fn a_page_that_replaced_the_built_ins_does_not_reach_the_helpers() {
    let (mut e, mut h) = (Jsc::new(), Quiet::default());
    let realm = e.new_realm();
    let r = run(&mut e, &mut h, realm, HOSTILE);
    e.release(r);
    // host microtasks queued with no script running go in at the
    // checkpoint through the realm's own `then`
    let f = run(&mut e, &mut h, realm, "(function (x) { globalThis.got = (globalThis.got | 0) + x; })");
    for _ in 0..3 {
        let job = e.retain(f.get());
        // SAFETY: a number carries no count
        e.enqueue_microtask(job, unsafe { Owned::from_raw(Jsc::number(2.0)) });
    }
    e.run_microtasks(&mut h);
    assert_eq!(number(&mut e, &mut h, realm, "got"), 6.0);
    assert_eq!(h.reported, 0);
    // weak handles, errors and a WindowProxy, made and read after it
    let w = e.weak(f.get());
    let got = e.weak_get(&w).expect("alive while held");
    e.release(got);
    e.weak_drop(w);
    let thrown = e.throw_error(ErrorKind::Type, "the host's own");
    let _sentinel = thrown.into_raw();
    let x = e.take_exception().expect("registered");
    let k = DomString::from_wtf8(b"message");
    let m = e.get(&mut h, x.get(), Key::Str(&k));
    assert!(e.as_string(m.get()).is_some(), "a real error, not the page's constructor");
    e.release(m);
    e.release(x);
    let p = e.new_window_proxy(realm);
    assert!(!Jsc::is_thrown(p.get()));
    let k = DomString::from_wtf8(b"got");
    let v = e.get(&mut h, p.get(), Key::Str(&k));
    assert_eq!(e.as_number(v.get()), Some(6.0), "read through the proxy");
    e.release(v);
    e.release(p);
    e.release(f);
}

#[test]
fn a_window_proxy_asked_for_while_halted_is_thrown_and_retargeting_needs_no_script() {
    let (mut e, mut h) = (Jsc::new(), Quiet::default());
    let (r1, r2) = (e.new_realm(), e.new_realm());
    let r = run(&mut e, &mut h, r2, "var which = 2; 0");
    e.release(r);
    let p = e.new_window_proxy(r1);
    e.interrupt_handle().terminate();
    // the stop is seen at the next entry
    let r = e.evaluate(&mut h, r1, &DomString::from_wtf8(b"0"), &ScriptOrigin { url: &DomString::from_wtf8(b"x"), line: 1, column: 0 });
    assert!(Jsc::is_thrown(r.into_raw()));
    let q = e.new_window_proxy(r1);
    assert!(Jsc::is_thrown(q.into_raw()), "making one runs script");
    e.set_window_proxy_target(p.get(), r2);
    assert_eq!(e.window_proxy_target(p.get()), Some(r2));
    e.release(p);
}
