//! NodeList and HTMLCollection as page script sees them on
//! JavaScriptCore: indices, liveness, iteration, and what a collection
//! keeps alive.

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
    let url = DomString::from_wtf8(b"collections");
    let realm = h.realm;
    let src = format!("String((function () {{ {src} }})())");
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    assert!(!Jsc::is_thrown(r.get()), "{src} threw");
    let mut b = Vec::new();
    e.as_string(r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    h.settle(e);
    String::from_utf8(b).unwrap()
}

fn collect_everything(e: &mut Jsc, h: &mut DomHost<Jsc>) {
    for _ in 0..3 {
        scrub();
        e.collect_now();
        while h.collect(e, Deadline::Work(u32::MAX)) != Step::Done {}
    }
}

#[test]
fn collections_answer_indices_and_iterate_as_arrays_do() {
    let (mut e, mut h) = page("<p>x</p>");
    let src = "var d = document.createElement('div');
for (const n of ['a', 'b', 'c']) { const s = d.appendChild(document.createElement('span')); s.id = n; }
d.insertBefore(document.createTextNode('t'), d.firstChild);
var nl = d.childNodes, hc = d.children, keys = [];
for (var k in nl) keys.push(k);
var each = []; nl.forEach(function (n, i) { each.push(i + n.nodeName); });
return [nl instanceof NodeList, hc instanceof HTMLCollection, nl.length, hc.length, nl[0] === d.firstChild, nl[4], 3 in nl, 4 in nl,
  Array.from(hc).map(n => n.id).join(''), [...nl].length, keys.slice(0, 4).join('|'), each.join(' '), typeof hc.forEach,
  nl.item(9), hc.namedItem('b') === hc[1], hc.namedItem('z'), Object.prototype.toString.call(hc), d.childNodes === nl,
  [...nl.keys()].join('')].join();";
    assert_eq!(
        eval(&mut e, &mut h, src),
        "true,true,4,3,true,,true,false,abc,4,0|1|2|3,0#text 1SPAN 2SPAN 3SPAN,undefined,,true,,[object HTMLCollection],true,0123"
    );
    h.close(&mut e);
}

#[test]
fn live_collections_follow_the_tree_and_query_results_do_not() {
    let (mut e, mut h) = page("<div id=r><b class='x y'></b><i class=x></i></div>");
    let src = "var r = document.getElementById('r'), xs = document.getElementsByClassName('x'), xy = r.getElementsByClassName(' y  x ');
var bs = document.getElementsByTagName('B'), all = r.getElementsByTagName('*'), q = r.querySelectorAll('.x'), kids = r.children;
var before = [xs.length, xy.length, bs.length, all.length, q.length, kids.length].join(' ');
r.appendChild(document.createElement('b')).setAttribute('class', 'x'); r.firstChild.setAttribute('class', 'y');
r.removeChild(r.lastChild.previousSibling);
return [before, [xs.length, xy.length, bs.length, all.length, q.length, kids.length].join(' '), q instanceof NodeList, xs[0].nodeName].join();";
    assert_eq!(eval(&mut e, &mut h, src), "2 1 1 2 2 2,1 0 2 2 2 2,true,B");
    h.close(&mut e);
}

#[test]
fn changing_attributes_or_text_through_a_collection_does_not_make_it_look_again() {
    let (mut e, mut h) = page("<ul id=u></ul>");
    let src = "var u = document.getElementById('u');
for (let i = 0; i < 2000; i++) u.appendChild(document.createElement('li')).appendChild(document.createTextNode('t'));
window.lis = document.getElementsByTagName('li'); window.kids = u.children; window.nodes = u.childNodes;
window.xs = u.getElementsByClassName('x');
return [lis.length, kids.length, nodes.length, xs.length].join();";
    assert_eq!(eval(&mut e, &mut h, src), "2000,2000,2000,0");
    let before = h.list_rebuilds();
    let src = "for (let i = 0; i < lis.length; i++) { lis[i].className = 'c' + i; kids[i].setAttribute('title', 't');
  kids[i].id = 'i' + i; nodes[i].firstChild.data = 'u'; kids[i].removeAttribute('title'); }
return [lis.length, kids.length, nodes.length].join();";
    assert_eq!(eval(&mut e, &mut h, src), "2000,2000,2000");
    assert_eq!(h.list_rebuilds(), before, "no collection looked again for attributes or text");
    // a class change makes a collection by class look again, once
    let src = "lis[7].className = 'x'; var a = xs.length; lis[8].id = 'q'; var b = xs.length; return [a, b, xs[0] === lis[7]].join();";
    assert_eq!(eval(&mut e, &mut h, src), "1,1,true");
    assert_eq!(h.list_rebuilds(), before + 1);
    // an insertion makes each one look again, once
    let src = "kids[0].parentNode.appendChild(document.createElement('li')); for (let i = 0; i < 100; i++) { lis[i].title = 'v'; lis.length; }
return [lis.length, kids.length, nodes.length, xs.length].join();";
    assert_eq!(eval(&mut e, &mut h, src), "2001,2001,2001,1");
    assert_eq!(h.list_rebuilds(), before + 5);
    h.close(&mut e);
}

#[test]
fn class_names_match_ascii_case_insensitively_only_in_quirks_mode() {
    let src = "var a = document.getElementsByClassName('fOO bar'), b = document.getElementsByClassName('\\u00c9t\\u00e9');
return [a.length, b.length, document.getElementsByClassName('BAR').length].join();";
    let html = "<div class='Foo BAR'></div><i class=bar></i><b class='\u{e9}t\u{e9}'></b>";
    for (doctype, want) in [("", "1,0,2"), ("<!DOCTYPE html>", "0,0,1")] {
        let (mut e, mut h) = page(&format!("{doctype}{html}"));
        assert_eq!(eval(&mut e, &mut h, src), want, "doctype {doctype:?}");
        h.close(&mut e);
    }
}

#[test]
fn sequences_and_fixed_lists_call_no_page_script_and_wrap_no_document_node() {
    let (mut e, mut h) = page("<ul id=u></ul>");
    let src = "var u = document.getElementById('u');
for (let i = 0; i < 500; i++) u.appendChild(document.createElement('li'));
window.calls = 0; Array.of = function () { calls++; return 'mine'; };
return 'ok';";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    let wrapped = h.dom.wrapped().len();
    let src = "var n = 0; for (let i = 0; i < 20; i++) n += document.querySelectorAll('li').length;
return [n, calls].join();";
    assert_eq!(eval(&mut e, &mut h, src), "10000,0");
    assert_eq!(h.dom.wrapped().len(), wrapped, "no document node wrapped for a fixed list");
    let src = "var el = document.createElement('p'); el.setAttribute('b', '1'); el.setAttribute('a', '2');
var names = el.getAttributeNames();
window.found = (function () { var d = document.createElement('div'); d.appendChild(document.createElement('i')).y = 'found';
  return d.querySelectorAll('i'); })();
return [Array.isArray(names), names.join(' '), Object.getPrototypeOf(names) === Array.prototype, calls].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,b a,true,0");
    collect_everything(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return [found[0].y, calls].join();"), "found,0");
    h.close(&mut e);
}

#[test]
fn a_fixed_list_keeps_the_tree_its_nodes_leave_the_document_for() {
    let (mut e, mut h) = page("<div id=c><ul id=u><li>a</li><li>b</li></ul><ol id=o><li>c</li></ol></div>");
    let src = "(function () { var c = document.getElementById('c'), u = c.firstChild, o = c.lastChild;
  window.nl = document.querySelectorAll('li'); u.x = 'ux'; nl[1].z = 'lz'; window.wm = new WeakMap([[nl[1], 'wm']]);
  o.y = 'oy'; u.remove(); o.remove(); c.appendChild(o); o.remove(); })();
// garbage for a collection before the host settles
for (let i = 0; i < 200000; i++) [i, i];
(function () { document.querySelectorAll('div'); })();
return 'ok';";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    collect_everything(&mut e, &mut h);
    let src = "document.getElementById('c').remove();
return [nl.length, nl[0].parentNode.x, nl[1].z, wm.get(nl[1]), nl[2].parentNode.y].join();";
    assert_eq!(eval(&mut e, &mut h, src), "3,ux,lz,wm,oy");
    h.close(&mut e);
}

#[test]
fn child_node_lists_are_the_same_object_for_as_long_as_their_root_wrapper_lives() {
    let (mut e, mut h) = page("<div id=r><b></b></div>");
    let (nodes, sides) = (h.dom.node_count(), h.side_objects());
    let src = "var r = document.getElementById('r'); r.childNodes.x = 'x'; r.children.y = 'y';
window.d = document.createElement('div'); d.appendChild(document.createElement('i')); d.childNodes.z = 'z';
for (let i = 0; i < 50; i++) { const t = document.createElement('div'); t.appendChild(document.createElement('i'));
  t.childNodes.w = i; t.children.w = i; }
return 'ok';";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    collect_everything(&mut e, &mut h);
    let src = "var r = document.getElementById('r'); return [r.childNodes.x, r.children.y, d.childNodes.z].join();";
    assert_eq!(eval(&mut e, &mut h, src), "x,y,z");
    // the dropped trees and their collections are freed: d and its child,
    // and the three collections of r and d
    let (n, s) = (h.dom.node_count() - nodes, h.side_objects() - sides);
    assert_eq!((n, s), (2, 3), "nodes and collections left");
    h.close(&mut e);
}

#[test]
fn indices_refuse_writes_and_an_html_collection_answers_names_its_prototype_does_not_have() {
    let (mut e, mut h) = page("<div id=r><b id=b1></b><i name=n1></i><a id=length></a><a name=item></a><p id=zz></p></div>");
    let src = "var r = document.getElementById('r'), nl = r.childNodes, hc = r.children, first = nl[0];
nl[7] = 'x'; nl[0] = 5; (function () { 'use strict'; nl[1] = 6; })();
var d = Object.getOwnPropertyDescriptor(nl, '0');
hc.zz = 'own'; hc.b1 = 5;
return [nl[0] === first, nl[7], Object.prototype.hasOwnProperty.call(nl, '7'), d.writable, d.value === first, 0 in nl, 9 in nl,
  hc.b1 === r.firstChild, hc.n1 === r.children[1], typeof hc.length, typeof hc.item, hc.zz === r.lastChild, Object.getOwnPropertyDescriptor(hc, 'b1').value === r.firstChild, hc.missing, 'b1' in hc, 'nope' in hc,
  r.getElementsByTagName('i').n1 === hc[1], hc[''], nl.b1].join();";
    assert_eq!(
        eval(&mut e, &mut h, src),
        "true,,false,false,true,true,false,true,true,number,function,true,true,,true,false,true,,"
    );
    h.close(&mut e);
}

#[test]
fn a_named_lookup_throws_what_the_prototype_chain_throws_and_guards_only_its_own_collection() {
    let (mut e, mut h) = page("<div id=r><b id=b1></b></div><div id=s><i id=i1></i></div>");
    let src = "var hc = document.getElementById('r').children, hs = document.getElementById('s').children, seen = [];
Object.setPrototypeOf(hc, new Proxy(Object.getPrototypeOf(hc), { has(t, k) {
  if (k === 'boom') throw new Error('trap');
  if (k === 'b1') seen.push(String(hs.i1 && hs.i1.id), String(hc.b1));
  return Reflect.has(t, k); } }));
var got; try { got = String(hc.boom); } catch (x) { got = x.message; }
return [got, hc.b1.id, seen.join(' ')].join();";
    assert_eq!(eval(&mut e, &mut h, src), "trap,b1,i1 undefined");
    h.close(&mut e);
}

#[inline(never)]
fn scrub() {
    let pad = [0u8; 512 * 1024];
    std::hint::black_box(&pad);
}
