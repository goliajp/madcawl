//! The interface objects on the global are made when first read: what
//! script can see of the global and its interfaces is as if they had
//! been made with the realm.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Engine, Key, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

const NAMES: &str = "AbortController,AbortSignal,Attr,NamedNodeMap,DOMTokenList,CSSStyleDeclaration,CharacterData,Text,\
Comment,DocumentFragment,CustomEvent,DOMException,EventTarget,Node,Element,Document,HTMLElement,HTMLUnknownElement,\
HTMLHtmlElement,HTMLHeadElement,HTMLBodyElement,HTMLDivElement,HTMLSpanElement,HTMLAnchorElement,HTMLLinkElement,\
HTMLStyleElement,HTMLImageElement,HTMLIFrameElement,HTMLObjectElement,HTMLButtonElement,HTMLInputElement,\
HTMLScriptElement,Event,NodeList,HTMLCollection,Location,History,Crypto,Performance,PerformanceObserver";

fn page() -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, b"<p>x</p>");
    (e, h)
}

/// The result of `src` as a string; panics with the message when it threw.
fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"lazy");
    let realm = h.realm;
    let src = format!("String((function () {{ {src} }})())");
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    assert!(!Jsc::is_thrown(r.get()), "{src} threw: {:?}", e.take_exception().map(|x| x.into_raw()));
    let mut b = Vec::new();
    e.to_string(h, r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    h.settle(e);
    String::from_utf8(b).unwrap()
}

#[test]
fn the_global_lists_every_interface_in_place_before_and_after_reads() {
    let (mut e, mut h) = page();
    let before = eval(&mut e, &mut h, "return Object.getOwnPropertyNames(window).join();");
    let at = before.find("AbortController").expect("the interfaces among the global's keys");
    assert!(before[at..].starts_with(NAMES), "{before}");
    let src = "var n = Object.getOwnPropertyNames(window); for (var i = n.length - 1; i >= 0; i--) window[n[i]];
return Object.getOwnPropertyNames(window).join();";
    assert_eq!(eval(&mut e, &mut h, src), before);
    h.close(&mut e);
}

#[test]
fn a_read_interface_is_a_writable_configurable_data_property_not_enumerable() {
    let (mut e, mut h) = page();
    let src = format!(
        "var bad = []; '{NAMES}'.split(',').forEach(function (n) {{
  var v = window[n], d = Object.getOwnPropertyDescriptor(window, n);
  if (typeof v !== 'function' || d.value !== v || !d.writable || d.enumerable || !d.configurable || 'get' in d || v.name !== n) bad.push(n);
}}); return bad.join() || 'ok';"
    );
    assert_eq!(eval(&mut e, &mut h, &src), "ok");
    h.close(&mut e);
}

#[test]
fn an_interface_not_yet_read_is_an_own_property() {
    let (mut e, mut h) = page();
    let src = "return ['HTMLSpanElement' in window, window.hasOwnProperty('HTMLSpanElement'), Object.hasOwn(globalThis, 'Crypto'),
  Object.keys(window).indexOf('PerformanceObserver') >= 0, 'NoSuchInterface' in window, typeof HTMLAnchorElement,
  Object.keys(window).indexOf('HTMLAnchorElement') >= 0, window.propertyIsEnumerable('Crypto')].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,true,true,false,false,function,false,false");
    h.close(&mut e);
}

#[test]
fn a_wrapper_made_first_shares_its_prototype_with_the_interface_read_later() {
    let (mut e, mut h) = page();
    let src = "var s = document.createElement('span'), p = document.querySelector('p');
return [Object.getPrototypeOf(s) === HTMLSpanElement.prototype, s instanceof HTMLSpanElement, s.constructor === HTMLSpanElement,
  s instanceof HTMLElement, s instanceof Node, p instanceof HTMLUnknownElement || p instanceof HTMLElement,
  document instanceof Document, Object.getPrototypeOf(document) === Document.prototype].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,true,true,true,true,true,true,true");
    h.close(&mut e);
}

#[test]
fn an_interface_is_the_same_object_on_every_read() {
    let (mut e, mut h) = page();
    let src = "var a = Node;
return [a === Node, Node === window.Node, self.Node === globalThis.Node, Object.getPrototypeOf(Element) === Node,
  Object.getPrototypeOf(Element.prototype) === Node.prototype, Object.getPrototypeOf(HTMLDivElement) === HTMLElement,
  Node.prototype.constructor === Node, new Event('x').constructor === Event].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,true,true,true,true,true,true,true");
    h.close(&mut e);
}

#[test]
fn writing_or_deleting_before_a_read_acts_on_the_data_property() {
    let (mut e, mut h) = page();
    let src = "Node = 5; delete window.Text; var o = Object.create(window); o.Comment = 1;
var d = Object.getOwnPropertyDescriptor(window, 'Node'), t = document.createTextNode('x');
return [Node, d.value, d.writable, d.enumerable, d.configurable, 'get' in d, 'Text' in window,
  Object.getPrototypeOf(t).constructor.name, Object.hasOwn(o, 'Comment'), o.Comment, typeof window.Comment].join();";
    assert_eq!(eval(&mut e, &mut h, src), "5,5,true,false,true,false,false,Text,true,1,function");
    h.close(&mut e);
}

#[test]
fn script_that_replaced_define_property_does_not_see_the_interfaces_made() {
    let (mut e, mut h) = page();
    let src = "var seen = 0; Object.defineProperty = Reflect.defineProperty = function () { seen++; };
var c = CustomEvent; return [seen, typeof c, Object.getOwnPropertyDescriptor(window, 'CustomEvent').value === c].join();";
    assert_eq!(eval(&mut e, &mut h, src), "0,function,true");
    h.close(&mut e);
}

#[test]
fn an_accessor_kept_after_its_realm_is_gone_throws_a_type_error() {
    let (mut e, mut old) = page();
    let url = DomString::from_wtf8(b"lazy");
    let src = DomString::from_wtf8(b"Object.getOwnPropertyDescriptor(window, 'Node').get");
    let realm = old.realm;
    let get = e.evaluate(&mut old, realm, &src, &ScriptOrigin { url: &url, line: 1, column: 0 });
    assert!(!Jsc::is_thrown(get.get()));
    old.close(&mut e);
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    let g = e.global(h.realm);
    assert!(e.set(&mut h, g.get(), Key::Str(&DomString::from_wtf8(b"kept")), get.get()));
    e.release(get);
    e.release(g);
    let src = "try { kept(); return 'no throw'; } catch (x) { return x.name + ': ' + x.message; }";
    assert_eq!(eval(&mut e, &mut h, src), "TypeError: the interface's window is gone");
    h.close(&mut e);
}

#[test]
fn a_saved_accessor_acts_on_the_property_once_it_is_made() {
    let (mut e, mut h) = page();
    let src = "var d = {}; ['Node', 'Text', 'Comment', 'Attr'].forEach(function (n) { d[n] = Object.getOwnPropertyDescriptor(window, n); });
var N = Node, T = Text; delete window.Node; var a = d.Node.get(), gone = !('Node' in window); Text = 1; var b = d.Text.get();
Object.defineProperty(window, 'Comment', { value: 2, writable: false }); d.Comment.set.call(window, 3);
delete window.Attr; var c = d.Attr.get(); d.Node.set.call(window, 4);
return [a, gone, b, Text, Comment, c, 'Attr' in window, Node, typeof N, typeof T].join();";
    assert_eq!(eval(&mut e, &mut h, src), ",true,1,1,2,,false,4,function,function");
    h.close(&mut e);
}

#[test]
fn a_sealed_global_keeps_its_unread_interfaces_writable() {
    let (mut e, mut h) = page();
    let src = "Object.seal(window); var c = CustomEvent; Node = 5; var o = Object.create(window); o.Comment = 1;
Object.defineProperty(window, 'Attr', { configurable: false }); var x = Attr; Attr = 6;
return [typeof c, CustomEvent === c, Node, window.Node, Object.hasOwn(o, 'Comment'), o.Comment, typeof Comment, typeof x, Attr].join();";
    assert_eq!(eval(&mut e, &mut h, src), "function,true,5,5,true,1,function,function,6");
    h.close(&mut e);
}

#[test]
fn a_frozen_global_keeps_its_unread_interfaces_read_only() {
    let (mut e, mut h) = page();
    let src = "Object.freeze(window); var n = Node; Node = 5; window.Text = 6; var o = Object.create(window); o.Comment = 1;
var strict = (function () { 'use strict'; try { Event = 7; return 'no throw'; } catch (x) { return x.name; } })();
return [typeof n, Node === n, typeof Text, Object.hasOwn(o, 'Comment'), typeof o.Comment, typeof Event, strict].join();";
    // a strict write would throw on the data property; the setter that
    // stands for it cannot tell strict code from sloppy
    assert_eq!(eval(&mut e, &mut h, src), "function,true,function,false,function,function,no throw");
    h.close(&mut e);
}

#[test]
fn an_interface_read_on_a_sealed_global_turns_read_only_once_it_is_frozen() {
    let (mut e, mut h) = page();
    let src = "Object.seal(window); var n = Node; Node = 5; var a = Node; Object.freeze(window); Node = 6;
return [typeof n, a, Node].join();";
    assert_eq!(eval(&mut e, &mut h, src), "function,5,5");
    h.close(&mut e);
}
