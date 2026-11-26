//! The DOM interfaces as page script sees them on JavaScriptCore:
//! constructors, prototype chains and the objects the bindings throw.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Engine, ScriptOrigin};
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

/// The result of `src` as a string; panics with the message when it threw.
fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"interfaces");
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

#[test]
fn dom_exceptions_are_objects_of_their_interface() {
    let (mut e, mut h) = page("<p>x</p>");
    let src = "var t; try { document.createElement('a b'); } catch (x) { t = x; }
var made = new DOMException('gone', 'NotFoundError'), plain = new DOMException();
return [t instanceof DOMException, t.constructor === DOMException, t.name, t.code, made.code, made.message,
  plain.name, plain.message === '', plain.code, DOMException.DATA_CLONE_ERR, made.SYNTAX_ERR].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,true,InvalidCharacterError,5,8,gone,Error,true,0,25,12");
    h.close(&mut e);
}

#[test]
fn custom_events_carry_their_detail_and_are_freed_with_it() {
    let (mut e, mut h) = page("<p>x</p>");
    let src = "var o = { n: 1 }, seen = []; var b = document.querySelector('body');
b.addEventListener('c', function (ev) { seen.push(ev.detail === o, ev instanceof CustomEvent, ev instanceof Event); });
var c = new CustomEvent('c', { detail: o, bubbles: true }); b.dispatchEvent(c);
var d = document.createEvent('CustomEvent'); var before = d.detail; d.initCustomEvent('c', false, false, 5);
return [seen.join(), c.bubbles, before, d.detail, new CustomEvent('c').detail, d.type].map(String).join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,true,true,true,null,5,null,c");
    let src = "for (let i = 0; i < 100; i++) { let c = new CustomEvent('c', { detail: {} }); c.detail.self = c; } return 'ok';";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    scrub();
    e.collect_now();
    while h.collect(&mut e, madcawl_js::Deadline::Work(u32::MAX)) != madcawl_js::Step::Done {}
    assert!(h.side_objects() < 10, "{} side objects left", h.side_objects());
    h.close(&mut e);
}

#[inline(never)]
fn scrub() {
    let pad = [0u8; 512 * 1024];
    std::hint::black_box(&pad);
}
