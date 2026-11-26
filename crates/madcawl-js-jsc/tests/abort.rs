//! AbortController and AbortSignal as page script uses them on
//! JavaScriptCore: the order an abort runs in, event handlers, the
//! `signal` option of `addEventListener`, timeouts, and what a
//! controller and its signal keep alive.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Deadline, Engine, ScriptOrigin, Step};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

fn page() -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, b"<p>x</p>");
    // DOMContentLoaded and load
    assert_eq!(h.run_tasks(&mut e, 10), 2);
    assert!(h.is_loaded());
    (e, h)
}

/// The result of `src` as a string, or the name of what it threw.
fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"abort");
    let realm = h.realm;
    let src = format!("String((function () {{ {src} }})())");
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    let r = if Jsc::is_thrown(r.get()) {
        let _sentinel = r.into_raw();
        e.take_exception().expect("an exception")
    } else {
        r
    };
    let mut b = Vec::new();
    e.to_string(h, r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    h.settle(e);
    String::from_utf8(b).unwrap()
}

#[inline(never)]
fn scrub() {
    let pad = [0u8; 512 * 1024];
    std::hint::black_box(&pad);
}

fn collect(e: &mut Jsc, h: &mut DomHost<Jsc>) {
    scrub();
    e.collect_now();
    while h.collect(e, Deadline::Work(u32::MAX)) != Step::Done {}
}

#[test]
fn an_abort_removes_the_signals_listeners_then_fires_its_event_once() {
    let (mut e, mut h) = page();
    let src = "var c = new AbortController(), s = c.signal, log = [], p = document.querySelector('p');
s.onabort = function (ev) { log.push('h:' + ev.type + ev.isTrusted + (this === s)); };
s.addEventListener('abort', function (ev) { log.push('l:' + (ev.target === s) + ev.eventPhase + ev.bubbles + ev.cancelable); });
s.addEventListener('abort', function () { log.push('gone'); }, { signal: s });
p.addEventListener('x', function () { log.push('x'); }, { signal: s });
p.dispatchEvent(new Event('x')); c.abort('why'); p.dispatchEvent(new Event('x')); c.abort('again');
var late = []; p.addEventListener('x', function () { late.push('late'); }, { signal: s }); p.dispatchEvent(new Event('x'));
return [log.join(), s.aborted, s.reason, c.signal === s, late.length, s instanceof EventTarget, Object.prototype.toString.call(s),
  Object.prototype.toString.call(c)].join('|');";
    let want = "x,h:aborttruetrue,l:true2falsefalse|true|why|true|0|true|[object AbortSignal]|[object AbortController]";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn an_event_handler_keeps_its_place_until_it_is_set_to_null() {
    let (mut e, mut h) = page();
    let src = "var c = new AbortController(), s = c.signal, log = [];
s.addEventListener('abort', () => log.push('a'));
s.onabort = () => log.push('h1');
s.addEventListener('abort', () => log.push('b'));
s.onabort = () => log.push('h2');
var h2 = s.onabort; s.removeEventListener('abort', h2);
c.abort();
var d = new AbortController(), t = d.signal, seen = [];
t.onabort = () => seen.push('h'); t.addEventListener('abort', () => seen.push('a')); t.onabort = null;
t.onabort = { handleEvent() { seen.push('object'); } }; var kept = typeof t.onabort; t.onabort = 'x';
t.onabort = () => seen.push('end'); d.abort();
return [log.join(), seen.join(), kept, s.onabort === h2, s.reason instanceof DOMException, s.reason.name].join('|');";
    assert_eq!(eval(&mut e, &mut h, src), "a,h2,b|a,end|object|true|true|AbortError");
    h.close(&mut e);
}

#[test]
fn an_event_handler_is_called_with_the_value_it_has_when_its_turn_comes() {
    let (mut e, mut h) = page();
    let src = "var c = new AbortController(), s = c.signal, log = [];
s.addEventListener('abort', () => { log.push('l'); s.onabort = () => log.push('replaced'); });
s.onabort = () => log.push('orig'); c.abort();
var d = new AbortController(), t = d.signal, seen = [];
t.addEventListener('abort', () => { t.onabort = null; t.onabort = () => seen.push('new'); });
t.onabort = () => seen.push('orig'); t.addEventListener('abort', () => seen.push('after')); d.abort();
return [log.join(), seen.join()].join('|');";
    assert_eq!(eval(&mut e, &mut h, src), "l,replaced|after");
    h.close(&mut e);
}

#[test]
fn signal_options_reasons_and_static_signals() {
    let (mut e, mut h) = page();
    let src = "var out = [], c = new AbortController(), p = document.querySelector('p'), n = 0;
var f = () => n++;
p.addEventListener('x', f, { signal: c.signal }); p.removeEventListener('x', f); p.addEventListener('x', f, { signal: c.signal });
c.abort(); p.dispatchEvent(new Event('x')); out.push(n);
for (const bad of [null, {}, 1, new EventTarget()]) { try { p.addEventListener('x', f, { signal: bad }); out.push('none'); } catch (x) { out.push(x.name); } }
var a = AbortSignal.abort(), b = AbortSignal.abort(undefined), r = {}, given = AbortSignal.abort(r);
out.push(a.aborted, a.reason.name, a.reason.code, b.reason.name, given.reason === r, AbortSignal.abort(null).reason);
try { given.throwIfAborted(); } catch (x) { out.push(x === r); }
out.push(new AbortController().signal.throwIfAborted(), new AbortController().signal.reason);
for (const ms of [-1, NaN, Infinity, 2 ** 53]) { try { AbortSignal.timeout(ms); out.push('none'); } catch (x) { out.push(x.name); } }
out.push(AbortSignal.length, AbortController.length, AbortSignal.timeout.length, AbortSignal.abort.length, typeof AbortSignal.prototype.timeout);
try { new AbortSignal(); } catch (x) { out.push(x.name); }
return out.map(String).join();";
    let want = "0,TypeError,TypeError,TypeError,TypeError,true,AbortError,20,AbortError,true,null,true,undefined,undefined,TypeError,TypeError,TypeError,TypeError,0,0,1,0,undefined,TypeError";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn a_timeout_signal_aborts_in_a_task_of_its_own_that_script_cannot_clear() {
    let (mut e, mut h) = page();
    let src = "globalThis.log = []; var t = AbortSignal.timeout(10);
t.onabort = (ev) => { log.push('abort:' + t.reason.name + ':' + t.reason.message); Promise.resolve().then(() => log.push('micro')); };
setTimeout(() => log.push('before'), 5); setTimeout(() => log.push('after'), 15);
for (let i = 1; i < 100; i++) clearTimeout(i); clearTimeout(2 ** 31);
setTimeout(() => log.push('before'), 5); setTimeout(() => log.push('after'), 15); return 'ok';";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    assert_eq!(h.run_tasks(&mut e, 100), 3);
    assert_eq!(eval(&mut e, &mut h, "return log.join();"), "before,abort:TimeoutError:signal timed out,micro,after");
    h.close(&mut e);
}

#[test]
fn a_controller_keeps_its_signals_properties_and_cycles_through_them_are_freed() {
    let (mut e, mut h) = page();
    let src = "globalThis.kept = new AbortController(); kept.signal.tag = 'kept';
for (let i = 0; i < 200; i++) {
  const c = new AbortController(), d = document.createElement('div');
  c.signal.c = c; d.addEventListener('x', () => c, { signal: c.signal }); c.signal.addEventListener('abort', () => d);
  c.signal.onabort = () => [c, d]; if (i % 2) c.abort(d);
}
return 'ok';";
    collect(&mut e, &mut h);
    let (nodes, sides) = (h.dom.node_count(), h.side_objects());
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    assert!(h.dom.node_count() >= nodes + 200 && h.side_objects() >= sides + 400, "the loop made its nodes and signals");
    collect(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return kept.signal.tag;"), "kept");
    collect(&mut e, &mut h);
    assert!(h.dom.node_count() <= nodes + 2, "{} nodes left of {nodes}", h.dom.node_count());
    assert!(h.side_objects() <= sides + 10, "{} side objects left of {sides}", h.side_objects());
    h.close(&mut e);
}
