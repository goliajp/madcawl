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

#[test]
fn event_targets_script_makes_dispatch_hold_and_free() {
    let (mut e, mut h) = page("<p>x</p>");
    let src = "var t = new EventTarget(), log = [];
t.addEventListener('x', function (ev) { log.push(this === t, ev.target === t, ev.currentTarget === t, ev.eventPhase); });
t.addEventListener('x', { handleEvent: function () { log.push('h'); } }, { capture: true });
var r = t.dispatchEvent(new Event('x', { bubbles: true }));
var bad; try { EventTarget.prototype.addEventListener.call({}, 'x', null); bad = 'none'; } catch (x) { bad = x.name; }
return [log.join(), r, t instanceof EventTarget, Object.getPrototypeOf(t) === EventTarget.prototype, bad].join('|');";
    assert_eq!(eval(&mut e, &mut h, src), "h,true,true,true,2|true|true|true|TypeError");
    let src = "var ev = globalThis.ev = new Event('x'); (function () { var u = new EventTarget(); u.x = 'kept'; u.dispatchEvent(ev); })();
for (let i = 0; i < 100; i++) { let u = new EventTarget(); u.addEventListener('x', () => u); } return 'ok';";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    let before = h.side_objects();
    scrub();
    e.collect_now();
    while h.collect(&mut e, madcawl_js::Deadline::Work(u32::MAX)) != madcawl_js::Step::Done {}
    assert_eq!(eval(&mut e, &mut h, "return globalThis.ev.target.x;"), "kept");
    assert!(h.side_objects() < before.saturating_sub(90), "{} of {before} side objects left", h.side_objects());
    h.close(&mut e);
}

#[test]
fn text_comment_and_fragment_interfaces() {
    let (mut e, mut h) = page("<p>x</p>");
    let src = "var t = document.createTextNode('hello'), out = [];
out.push(t instanceof Text, t instanceof CharacterData, t instanceof Node, t.length, t.substringData(1, 3));
t.appendData(' world'); t.insertData(0, '>'); t.deleteData(1, 1); t.replaceData(0, 1, '[');
out.push(t.data); var bad; try { t.substringData(99, 1); } catch (x) { bad = x.name; } out.push(bad);
var p = document.querySelector('p'); p.appendChild(t); var rest = t.splitText(5);
out.push(t.data, rest.data, rest.previousSibling === t, t.wholeText, rest.wholeText === t.wholeText);
t.data = null; out.push(JSON.stringify(t.data), new Text().data === '', new Text('a').ownerDocument === document);
var c = new Comment('c'); out.push(c instanceof Comment, c.nodeType, document.createComment('d').data);
var f = new DocumentFragment(); f.appendChild(document.createElement('b')).id = 'q'; f.appendChild(new Text('z'));
out.push(f instanceof DocumentFragment, f.getElementById('q').tagName, f.querySelectorAll('b').length);
p.appendChild(f); out.push(f.firstChild, p.lastChild.data, document.createDocumentFragment().nodeType);
c.remove(); p.appendChild(c); c.remove(); out.push(c.parentNode);
return out.map(String).join();";
    let want = "true,true,true,5,ell,[ello world,IndexSizeError,[ello, world,true,x[ello world,true,\"\",true,true,true,8,d,true,B,1,null,z,11,null";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn subclasses_of_the_constructible_interfaces_get_their_own_prototype() {
    let (mut e, mut h) = page("<p>x</p>");
    let src = "class A extends EventTarget { foo() { return 'a'; } }
class B extends Event { constructor(t) { super(t, { bubbles: true }); this.own = 1; } bar() { return 'b'; } }
class C extends CustomEvent {} class T extends Text {}
var a = new A(), b = new B('y'), c = new C('x', { detail: 3 }), t = new T('z'), seen = [];
a.addEventListener('y', function (ev) { seen.push(ev.bar(), this.foo(), ev.target === a); });
a.dispatchEvent(b);
var R = Reflect.construct(Event, ['r'], Object);
var calls = []; for (const f of [EventTarget, Event, Text]) { try { f('q'); calls.push('none'); } catch (x) { calls.push(x.name); } }
return [a.foo(), a instanceof A, a instanceof EventTarget, Object.getPrototypeOf(b) === B.prototype, b.bubbles, b.own,
  seen.join(), Object.getPrototypeOf(c) === C.prototype, c.detail, c instanceof CustomEvent,
  Object.getPrototypeOf(t) === T.prototype, t.data, t instanceof Text, t.nodeType,
  Object.getPrototypeOf(R) === Object.prototype, new Event('p') instanceof B, calls.join(' '),
  Object.getOwnPropertyDescriptor(Event, 'prototype').writable].join();";
    let want = "a,true,true,true,true,1,b,a,true,true,3,true,true,z,true,3,true,false,TypeError TypeError TypeError,false";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[inline(never)]
fn scrub() {
    let pad = [0u8; 512 * 1024];
    std::hint::black_box(&pad);
}
