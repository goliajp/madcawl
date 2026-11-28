//! Iframes on JavaScriptCore: a child navigable with a window in a realm
//! of its own, `contentWindow` and `contentDocument`, the window tree
//! (`parent`, `top`, `frames`, `length`, `frameElement`), `load` on
//! insertion, `srcdoc` and a `src` of the same origin, and documents of
//! another origin, whose window script reaches only through a stand-in.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Engine, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

fn page(html: &str) -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    assert!(h.set_url("https://a.test/dir/page"));
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, html.as_bytes());
    while h.run_tasks(&mut e, 100) != 0 {}
    (e, h)
}

/// `src` run as a function body, its result as a string.
fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"iframes");
    let realm = h.realm;
    let src = format!("String((function () {{ try {{ {src} }} catch (x) {{ return 'threw ' + x; }} }})())");
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    assert!(!Jsc::is_thrown(r.get()), "{src} threw: {:?}", h.take_notes());
    let mut b = Vec::new();
    e.to_string(h, r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    h.settle(e);
    String::from_utf8(b).unwrap()
}

fn settle_tasks(e: &mut Jsc, h: &mut DomHost<Jsc>) {
    while h.run_tasks(e, 100) != 0 {}
}

#[test]
fn an_inserted_iframe_has_a_window_and_a_document_of_its_own() {
    let (mut e, mut h) = page("<body>");
    let src = "var f = document.createElement('iframe'); var before = [f.contentWindow, f.contentDocument].join();
document.body.appendChild(f); var w = f.contentWindow, d = f.contentDocument;
return [before, d.documentElement.outerHTML, d.defaultView === w, w.document === d, w.frameElement === f,
  d instanceof w.Document, d instanceof Document, w.Array === Array, d.readyState,
  Object.getOwnPropertyDescriptor(f, 'contentWindow') === undefined].join();";
    let want = ",,<html><head></head><body></body></html>,true,true,true,true,false,false,complete,true";
    assert_eq!(eval(&mut e, &mut h, src), want);
    let src = "var d = document.querySelector('iframe').contentDocument; var p = d.createElement('p'); d.body.appendChild(p);
var s = d.createElement('script'); s.textContent = 'parent.ran = 1'; d.body.appendChild(s);
return [p instanceof d.defaultView.HTMLElement, p instanceof HTMLElement, p.ownerDocument === d, d.body.childNodes.length, typeof ran].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,false,true,2,undefined");
    h.close(&mut e);
}

#[test]
fn the_window_tree_reads_the_same_from_both_sides() {
    let (mut e, mut h) = page("<body>");
    let src = "var f = document.body.appendChild(document.createElement('iframe')); var w = f.contentWindow;
var g = w.document.body.appendChild(w.document.createElement('iframe')).contentWindow;
return [window.length, frames === window, window.frameElement, top === window, parent === window,
  w.parent === window, w.top === window, w.frames === w.window, w.length, w.opener,
  g.parent === w, g.top === window, g.parent.parent === window, g.frameElement.ownerDocument === w.document].join();";
    assert_eq!(eval(&mut e, &mut h, src), "1,true,,true,true,true,true,true,1,,true,true,true,true");
    h.close(&mut e);
}

#[test]
fn an_about_blank_iframe_fires_load_as_it_is_inserted() {
    let (mut e, mut h) = page("<body>");
    let src = "var log = []; var f = document.createElement('iframe'); f.onload = () => log.push('onload');
f.addEventListener('load', ev => log.push(ev.type + ':' + ev.bubbles + ':' + (f.contentDocument !== null)));
log.push('before'); document.body.appendChild(f); log.push('after');
var g = document.createElement('iframe'); g.src = 'about:blank#x'; g.onload = () => log.push('blank'); document.body.appendChild(g);
return log.join();";
    assert_eq!(eval(&mut e, &mut h, src), "before,onload,load:false:true,after,blank");
    h.close(&mut e);
}

#[test]
fn iframes_the_parser_inserts_have_their_windows_before_the_next_script() {
    let html = "<iframe id=a></iframe><div><iframe id=b srcdoc='<p>x'></iframe></div><script>var a = document.getElementById('a'), b = document.getElementById('b');
var seen = [window.length, a.contentDocument.body.localName, b.contentDocument.body.childNodes.length].join();</script>";
    let (mut e, mut h) = page(html);
    assert_eq!(eval(&mut e, &mut h, "return seen;"), "2,body,0");
    assert_eq!(eval(&mut e, &mut h, "return b.contentDocument.body.innerHTML;"), "<p>x</p>");
    h.close(&mut e);
}

#[test]
fn srcdoc_loads_in_a_task_in_the_same_window_without_running_its_scripts() {
    let (mut e, mut h) = page("<body>");
    let src = "window.log = []; var f = document.createElement('iframe');
f.srcdoc = '<title>t</title><a href=x>a</a><script>parent.log.push(\"ran\")</script><iframe srcdoc=\"<b>in</b>\"></iframe>';
f.onload = () => log.push('load:' + f.contentDocument.title); document.body.appendChild(f);
window.first = f.contentWindow; window.blank = f.contentDocument; log.push(f.contentDocument.body.childNodes.length);
return log.join();";
    assert_eq!(eval(&mut e, &mut h, src), "0");
    settle_tasks(&mut e, &mut h);
    let src = "var f = document.querySelector('iframe'), d = f.contentDocument;
return [log.join(), f.contentWindow === first, d !== blank, d.defaultView === first, first.document === d,
  d.querySelector('a').href, blank.childNodes.length, d.querySelector('iframe').contentDocument.body.innerHTML, window.length, first.length].join();";
    assert_eq!(eval(&mut e, &mut h, src), "0,load:t,true,true,true,true,https://a.test/dir/x,0,<b>in</b>,1,1");
    h.close(&mut e);
}

#[test]
fn a_src_of_the_same_origin_is_fetched_through_the_embedder() {
    let (mut e, mut h) = page("<body>");
    let src = "window.log = []; var f = document.createElement('iframe'); f.src = 'sub/doc?q'; f.onload = () => log.push('load');
document.body.appendChild(f); var g = document.createElement('iframe'); g.src = 'https://a.test/gone'; g.onload = () => log.push('gone'); document.body.appendChild(g);
return log.join() + '|' + f.contentDocument.body.childNodes.length;";
    assert_eq!(eval(&mut e, &mut h, src), "|0");
    let requests = h.take_frame_requests();
    let urls: Vec<&str> = requests.iter().map(|(_, u)| u.as_str()).collect();
    assert_eq!(urls, ["https://a.test/dir/sub/doc?q", "https://a.test/gone"]);
    h.frame_fetched(requests[0].0, Some(("https://a.test/dir/sub/doc?q", b"<p>fetched</p><a href=z>z</a>")));
    h.frame_fetched(requests[1].0, None);
    settle_tasks(&mut e, &mut h);
    let src = "var [f, g] = document.querySelectorAll('iframe');
return [log.join(), f.contentDocument.body.innerHTML.slice(0, 14), f.contentDocument.querySelector('a').href, g.contentDocument.body.childNodes.length].join();";
    assert_eq!(eval(&mut e, &mut h, src), "load,gone,<p>fetched</p>,https://a.test/dir/sub/z,0");
    h.close(&mut e);
}

#[test]
fn a_document_of_another_origin_is_not_reached() {
    let (mut e, mut h) = page("<body>");
    let src = "var log = []; var f = document.createElement('iframe'); f.src = 'https://b.test/'; f.onload = () => log.push('load');
document.body.appendChild(f); var w = f.contentWindow;
var s = document.createElement('iframe'); s.setAttribute('sandbox', 'allow-scripts'); document.body.appendChild(s);
var t = document.createElement('iframe'); t.setAttribute('sandbox', ' ALLOW-same-origin '); document.body.appendChild(t);
return [f.contentDocument, w === f.contentWindow, w.window === w, w.self === w, w.frames === w, w.parent === window, w.top === window,
  w.opener, w.closed, w.length, Object.keys(w).join(' '), s.contentDocument, s.contentWindow.parent === window,
  t.contentDocument !== null, t.contentWindow.document === t.contentDocument, log.join()].join();";
    let keys = "window self frames parent top opener closed length postMessage";
    assert_eq!(eval(&mut e, &mut h, src), format!(",true,true,true,true,true,true,,false,0,{keys},,true,true,true,"));
    assert!(h.take_frame_requests().is_empty(), "another origin is not fetched");
    let src = "var w = document.querySelector('iframe').contentWindow; document.querySelector('iframe').remove();
return [w.closed, w.parent, w.top].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,,");
    h.close(&mut e);
}

#[test]
fn a_redirect_to_another_origin_leaves_the_document_unloaded() {
    let (mut e, mut h) = page("<body>");
    let src = "window.log = []; var f = document.createElement('iframe'); f.src = '/r'; f.onload = () => log.push('load');
document.body.appendChild(f); return '';";
    eval(&mut e, &mut h, src);
    let (id, url) = h.take_frame_requests().remove(0);
    assert_eq!(url, "https://a.test/r");
    h.frame_fetched(id, Some(("https://c.test/landed", b"<p>elsewhere")));
    assert_eq!(eval(&mut e, &mut h, "return log.join();"), "", "load comes in a task");
    settle_tasks(&mut e, &mut h);
    let src = "var f = document.querySelector('iframe'); return [f.contentDocument, f.contentWindow.closed, log.join()].join();";
    assert_eq!(eval(&mut e, &mut h, src), ",false,load");
    h.close(&mut e);
}

/// An iframe whose document is of another origin, which is not loaded
/// here, still fires `load` in a task, and the document it is in waits
/// for it, as when that document loads in a browser.
#[test]
fn an_iframe_of_another_origin_fires_load_and_delays_its_document_s() {
    let html = "<iframe src='https://b.test/'></iframe><iframe sandbox src=same></iframe><iframe src=r></iframe>
<script>window.log = []; addEventListener('load', () => log.push('window'));
document.querySelectorAll('iframe').forEach((f, i) => f.onload = () => log.push(i + ':' + f.contentDocument));</script>";
    let (mut e, mut h) = page(html);
    let requests = h.take_frame_requests();
    let urls: Vec<&str> = requests.iter().map(|(_, u)| u.as_str()).collect();
    assert_eq!(urls, ["https://a.test/dir/r"], "only the same origin's is fetched");
    assert_eq!(eval(&mut e, &mut h, "return log.join() + ',' + document.readyState;"), "0:null,1:null,interactive");
    h.frame_fetched(requests[0].0, Some(("https://c.test/landed", b"<p>elsewhere")));
    settle_tasks(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return log.join() + ',' + document.readyState;"), "0:null,1:null,2:null,window,complete");
    h.close(&mut e);
}

/// An error a host function throws is made in the realm of that
/// function, the standard's current realm, not in the realm a wrapper
/// was last made in.
#[test]
fn an_error_a_host_function_throws_is_of_that_function_s_realm() {
    let (mut e, mut h) = page("<body>");
    let src = "var f = document.body.appendChild(document.createElement('iframe')); var w = f.contentWindow;
function err(fn) { try { fn(); } catch (x) { return x; } }
var theirs = err(() => w.queueMicrotask(1)), ours = err(() => { f.contentDocument.body; queueMicrotask(1); });
return [theirs instanceof w.TypeError, theirs instanceof TypeError, ours instanceof TypeError, ours instanceof w.TypeError].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,false,true,false");
    h.close(&mut e);
}

#[test]
fn a_window_of_another_origin_denies_every_other_member() {
    let (mut e, mut h) = page("<body>");
    let src = "var f = document.createElement('iframe'); f.src = 'https://b.test/'; document.body.appendChild(f); var w = f.contentWindow;
function name(fn) { try { fn(); return 'none'; } catch (x) { return x.name; } }
var denied = [() => w.document, () => w.foo, () => { w.foo = 1; }, () => 'foo' in w, () => delete w.window,
  () => Object.defineProperty(w, 'x', { value: 1 }), () => String(w), () => w[Symbol.iterator]].map(name);
var leak = [() => { 'use strict'; Object.getOwnPropertyDescriptor(w, 'window').get.leak = 1; },
  () => { 'use strict'; w.postMessage.leak = 1; }].map(name);
return [denied.join(' '), leak.join(' '), w.then, w[Symbol.toStringTag], Object.getPrototypeOf(w), 'window' in w,
  w.postMessage('hi', '*'), w.postMessage.length, f.contentWindow === w].join();";
    let denied = ["SecurityError"; 8].join(" ");
    assert_eq!(eval(&mut e, &mut h, src), format!("{denied},TypeError TypeError,,,,true,,1,true"));
    let src = "var w = document.querySelector('iframe').contentWindow;
var quiet = Reflect.ownKeys(w).slice(-4).map(String).join(' ');
var d = Object.getOwnPropertyDescriptor(w, 'then'), s = Object.getOwnPropertyDescriptor(w, Symbol.hasInstance);
return [quiet, 'then' in w, Symbol.toStringTag in w, Symbol.isConcatSpreadable in w, 'value' in d, d.value, d.writable, d.enumerable,
  d.configurable, s.configurable, Object.keys(w).includes('then'), Promise.resolve(w).then(x => x === w) instanceof Promise].join();";
    let quiet = "then Symbol(Symbol.toStringTag) Symbol(Symbol.hasInstance) Symbol(Symbol.isConcatSpreadable)";
    assert_eq!(eval(&mut e, &mut h, src), format!("{quiet},true,true,true,true,,false,false,true,true,false,true"));
    h.close(&mut e);
}

#[test]
fn an_iframe_removed_before_its_document_came_asks_for_nothing_more() {
    let (mut e, mut h) = page("<body>");
    let src = "for (var i = 0; i < 1000; i++) { var f = document.createElement('iframe'); f.src = 'doc' + i; document.body.appendChild(f); f.remove(); }
window.log = []; var g = document.createElement('iframe'); g.src = 'kept'; g.onload = () => log.push(g.contentDocument.body.textContent);
document.body.appendChild(g); var late = document.createElement('iframe'); late.src = 'late'; document.body.appendChild(late); return '';";
    eval(&mut e, &mut h, src);
    let requests = h.take_frame_requests();
    let urls: Vec<&str> = requests.iter().map(|(_, u)| u.as_str()).collect();
    assert_eq!(urls, ["https://a.test/dir/kept", "https://a.test/dir/late"]);
    eval(&mut e, &mut h, "document.querySelectorAll('iframe')[1].remove(); return '';");
    h.frame_fetched(requests[1].0, Some(("https://a.test/dir/late", b"<p>late")));
    h.frame_fetched(requests[0].0, Some(("https://a.test/dir/kept", b"<p>kept")));
    settle_tasks(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return [log.join(), window.length].join();"), "kept,1");
    assert!(h.take_frame_requests().is_empty());
    h.close(&mut e);
}

/// The window's `load` waits for an iframe still loading its document,
/// which waits in turn for the iframes in that document; one removed
/// while loading delays nothing more.
#[test]
fn an_iframe_loading_delays_the_load_of_the_document_it_is_in() {
    let html = "<iframe src=a></iframe><iframe src=gone></iframe><script>window.log = []; addEventListener('load', () => log.push('load'));
document.querySelector('iframe').onload = function () { log.push('a:' + this.contentDocument.querySelectorAll('iframe')[1].contentDocument.body.textContent); };</script>";
    let (mut e, mut h) = page(html);
    let requests = h.take_frame_requests();
    assert_eq!(requests.len(), 2);
    let src = "document.querySelectorAll('iframe')[1].remove(); return log.join() + document.readyState;";
    assert_eq!(eval(&mut e, &mut h, src), "interactive");
    settle_tasks(&mut e, &mut h);
    h.frame_fetched(requests[0].0, Some(("https://a.test/dir/a", b"<iframe srcdoc='<i>s</i>'></iframe><iframe src=b></iframe>")));
    settle_tasks(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return log.join() + document.readyState;"), "interactive");
    let (id, url) = h.take_frame_requests().remove(0);
    assert_eq!(url, "https://a.test/dir/b");
    h.frame_fetched(id, Some(("https://a.test/dir/b", b"<b>inner</b>")));
    settle_tasks(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return log.join() + ',' + document.readyState;"), "a:inner,load,complete");
    h.close(&mut e);
}
