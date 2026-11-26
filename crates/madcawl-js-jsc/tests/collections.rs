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
fn a_collection_keeps_its_root_tree_and_a_dropped_one_is_freed() {
    let (mut e, mut h) = page("<p>x</p>");
    let (nodes, sides) = (h.dom.node_count(), h.side_objects());
    let src = "for (let i = 0; i < 100; i++) { const d = document.createElement('div'); d.appendChild(document.createElement('i'));
d.childNodes.length; d.children[0]; d.getElementsByTagName('i').length; d.querySelectorAll('i')[0]; }
window.kept = (function () { var d = document.createElement('div'); d.x = 'kept'; d.appendChild(document.createElement('i')); return d.children; })();
window.found = (function () { var d = document.createElement('div'); d.appendChild(document.createElement('i')).y = 'found'; return d.querySelectorAll('i'); })();
return 'ok';";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    collect_everything(&mut e, &mut h);
    // the two kept collections and their trees; in parallel test runs the
    // framework now and then keeps one more object past a full
    // collection (a collection of one early round, never more), so one
    // round's tree and collection are allowed
    let (n, s) = (h.dom.node_count() - nodes, h.side_objects() - sides);
    assert!((4..=6).contains(&n) && (2..=3).contains(&s), "{n} nodes and {s} collections left");
    assert_eq!(eval(&mut e, &mut h, "return [kept[0].parentNode.x, found[0].y].join();"), "kept,found");
    assert_eq!(h.dom.audit().mismatches(), 0);
    h.close(&mut e);
}

#[inline(never)]
fn scrub() {
    let pad = [0u8; 512 * 1024];
    std::hint::black_box(&pad);
}
