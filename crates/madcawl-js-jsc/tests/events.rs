//! Events made and dispatched by script on JavaScriptCore: the order of
//! the phases, stopping, canceling, the listener options, and freeing
//! the events script lets go of.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Deadline, Engine, ScriptOrigin, Step};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

fn page(html: &str) -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, html.as_bytes());
    (e, h)
}

fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"events");
    let realm = h.realm;
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    if Jsc::is_thrown(r.get()) {
        let _sentinel = r.into_raw();
        let x = e.take_exception().expect("an exception");
        let m = e.to_string(h, x.get()).expect("a message");
        let mut b = Vec::new();
        m.write_wtf8(&mut b);
        panic!("threw {}", String::from_utf8_lossy(&b));
    }
    let mut b = Vec::new();
    e.as_string(r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    h.settle(e);
    String::from_utf8(b).unwrap()
}

const TREE: &str = "var log = []; var b = document.querySelector('body'); var d = document.createElement('div'); b.appendChild(d);\
var s = document.createElement('span'); d.appendChild(s);\
function L(name) { return function (e) { log.push(name + ':' + e.eventPhase + ':' + (e.currentTarget === this)); }; }";

#[test]
fn capture_then_target_then_bubble_up_to_the_window() {
    let (mut e, mut h) = page("<p>x</p>");
    let src = format!("{TREE}
document.addEventListener('x', L('doc-c'), true); d.addEventListener('x', L('div-c'), true);
d.addEventListener('x', L('div-b')); s.addEventListener('x', L('span-b')); s.addEventListener('x', L('span-c'), true);
window.addEventListener('x', function () {{ log.push('win'); }});
var ev = new Event('x', {{ bubbles: true }}); var r = s.dispatchEvent(ev);
[log.join(), r, ev.target === s, ev.eventPhase, ev.currentTarget, ev instanceof Event, ev.composedPath().length, d instanceof Element].join('|')");
    let want = "doc-c:1:true,div-c:1:true,span-c:2:true,span-b:2:true,div-b:3:true,win|true|true|0||true|0|true";
    assert_eq!(eval(&mut e, &mut h, &src), want);
    // not bubbling: the capture phase and the target only
    let src = "log = []; s.dispatchEvent(new Event('x')); log.join()";
    assert_eq!(eval(&mut e, &mut h, src), "doc-c:1:true,div-c:1:true,span-c:2:true,span-b:2:true");
    h.close(&mut e);
}

#[test]
fn stopping_canceling_once_and_handle_event() {
    let (mut e, mut h) = page("<p>x</p>");
    let src = format!("{TREE}
d.addEventListener('x', function (e) {{ log.push('d'); e.stopPropagation(); }}, true);
s.addEventListener('x', L('never'));
var ev = new Event('x', {{ bubbles: true, cancelable: true }});
d.addEventListener('y', function (e) {{ e.preventDefault(); log.push('cancel'); }});
d.addEventListener('z', {{ handleEvent: function (e) {{ log.push('handle:' + (this !== d)); }} }}, {{ once: true }});
d.addEventListener('w', function (e) {{ log.push('w1'); e.stopImmediatePropagation(); }}); d.addEventListener('w', L('w2'));
var a = s.dispatchEvent(ev);
var c = d.dispatchEvent(new Event('y', {{ cancelable: true }}));
var n = d.dispatchEvent(new Event('y'));
d.dispatchEvent(new Event('z')); d.dispatchEvent(new Event('z'));
d.dispatchEvent(new Event('w'));
[log.join(), a, c, n, ev.defaultPrevented, ev.cancelBubble].join('|')");
    assert_eq!(eval(&mut e, &mut h, &src), "d,cancel,cancel,handle:true,w1|true|false|true|false|false");
    let src = "var t; try { var x = new Event('q'); d.addEventListener('q', function () { d.dispatchEvent(x); }); d.dispatchEvent(x); t = 'none'; } catch (err) { t = err.name; } t";
    assert_eq!(eval(&mut e, &mut h, src), "none", "a listener's exception is reported, not thrown");
    assert_eq!(h.reported, 1, "InvalidStateError from the nested dispatch");
    h.close(&mut e);
}

#[test]
fn events_script_lets_go_of_are_freed() {
    let (mut e, mut h) = page("<p>x</p>");
    let src = format!("{TREE} d.addEventListener('x', function (e) {{ e.kept = e.target; }});
for (let i = 0; i < 500; i++) s.dispatchEvent(new Event('x', {{ bubbles: true }})); 'ok'");
    assert_eq!(eval(&mut e, &mut h, &src), "ok");
    scrub();
    e.collect_now();
    while h.collect(&mut e, Deadline::Work(u32::MAX)) != Step::Done {}
    assert!(h.side_objects() < 10, "{} events left", h.side_objects());
    assert_eq!(h.dom.audit().mismatches(), 0);
    h.close(&mut e);
}

#[inline(never)]
fn scrub() {
    let pad = [0u8; 512 * 1024];
    std::hint::black_box(&pad);
}
