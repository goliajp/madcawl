//! How a child navigable ends on JavaScriptCore: when its iframe leaves
//! the document its realm is detached, what script still holds of it
//! throws, its timers and listeners go, its nodes are freed, and none of
//! it piles up over many iframes; a host caps how many live at once.

use madcawl_bindings::{DomHost, Note};
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
    while h.run_tasks(&mut e, 100) != 0 {}
    (e, h)
}

fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"iframe-lifetime");
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

fn scrub() {
    let pad = [0u8; 512 * 1024];
    std::hint::black_box(&pad);
}

fn collect(e: &mut Jsc, h: &mut DomHost<Jsc>) {
    for _ in 0..3 {
        scrub();
        e.collect_now();
        while h.collect(e, Deadline::Work(u32::MAX)) != Step::Done {}
    }
}

#[test]
fn removing_the_iframe_detaches_its_window() {
    let (mut e, mut h) = page("<body>");
    let src = "var f = document.body.appendChild(document.createElement('iframe'));
window.w = f.contentWindow; window.d = f.contentDocument; d.body.appendChild(d.createElement('p'));
var g = d.body.appendChild(d.createElement('iframe')); window.gw = g.contentWindow;
f.remove();
function throws(fn) { try { fn(); return 'no'; } catch (x) { return x.name; } }
return [f.contentWindow, f.contentDocument, window.length, throws(() => d.body),
  throws(() => d.createElement('p')), throws(() => w.Node), throws(() => gw.Node)].join();";
    assert_eq!(eval(&mut e, &mut h, src), ",,0,TypeError,TypeError,TypeError,TypeError");
    assert_eq!(e.live_realms(), 1, "the child windows' realms are detached");
    let src = "var f = document.querySelector('body').appendChild(document.createElement('iframe'));
return [f.contentWindow !== w, f.contentDocument.body.localName, window.length].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,body,1");
    h.close(&mut e);
}

#[test]
fn a_window_that_goes_takes_its_timers_and_listeners() {
    let (mut e, mut h) = page("<body>");
    let src = "window.log = []; var f = document.body.appendChild(document.createElement('iframe')); var w = f.contentWindow;
var mine = setTimeout(() => log.push('top'), 5);
w.clearTimeout(mine); w.setTimeout(() => log.push('child'), 10); w.setInterval(() => log.push('tick'), 1);
w.setTimeout('parent.log.push(typeof Node === typeof parent.Node && Node !== parent.Node)', 0);
w.addEventListener('x', () => log.push('x'));
return '';";
    eval(&mut e, &mut h, src);
    while h.run_tasks(&mut e, 1) != 0 && eval(&mut e, &mut h, "return log.filter(x => x == 'tick').length < 3;") == "true" {}
    let src = "var w = document.querySelector('iframe').contentWindow; document.querySelector('iframe').remove(); return log.join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,tick,tick,tick");
    for _ in 0..50 {
        h.run_tasks(&mut e, 100);
    }
    assert_eq!(eval(&mut e, &mut h, "return log.join();"), "true,tick,tick,tick,top");
    assert!(h.next_work_in().is_none(), "no timer of the child is left");
    h.close(&mut e);
}

#[test]
fn many_iframes_made_and_removed_leave_nothing_behind() {
    let (mut e, mut h) = page("<body><p>x</p>");
    let src = "var f = document.body.appendChild(document.createElement('iframe')); f.contentDocument.body.innerHTML = '<b>w</b>'; f.remove(); return '';";
    eval(&mut e, &mut h, src);
    collect(&mut e, &mut h);
    let nodes = h.dom.node_count();
    let src = "for (var i = 0; i < 300; i++) { var f = document.createElement('iframe'); f.setAttribute('sandbox', 'allow-same-origin');
document.body.appendChild(f); var w = f.contentWindow, d = f.contentDocument;
if (d.defaultView !== w || w.frameElement !== f || w.document !== d) throw new Error('at ' + i);
d.body.appendChild(d.createElement('div')).textContent = i; f.remove(); }
return 'ok';";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    collect(&mut e, &mut h);
    assert_eq!(e.live_realms(), 1);
    assert_eq!(h.dom.node_count(), nodes, "the child documents are freed");
    assert_eq!(h.dom.audit().mismatches(), 0);
    h.close(&mut e);
}

#[test]
fn a_host_keeps_a_bounded_number_of_navigables_and_says_when_it_refuses() {
    let (mut e, mut h) = page("<body>");
    let src = "var n = 0; for (var i = 0; i < 130; i++) { var f = document.body.appendChild(document.createElement('iframe')); if (f.contentWindow) n++; }
return [n, window.length].join();";
    assert_eq!(eval(&mut e, &mut h, src), "128,128");
    let notes = h.take_notes();
    assert_eq!(notes.iter().filter(|&n| *n == Note::FrameLimit).count(), 2);
    let src = "document.querySelector('iframe').remove(); var f = document.body.appendChild(document.createElement('iframe')); return f.contentWindow !== null;";
    assert_eq!(eval(&mut e, &mut h, src), "true");
    h.close(&mut e);
}

#[test]
fn a_page_with_iframes_closes_and_gives_back_its_dom() {
    let (mut e, mut h) = page("<iframe srcdoc='<iframe></iframe>'></iframe><iframe></iframe>");
    assert_eq!(eval(&mut e, &mut h, "return window.length + ':' + frames.length;"), "2:2");
    let dom = h.into_dom(&mut e);
    assert_eq!(dom.audit().mismatches(), 0);
    assert_eq!(e.live_realms(), 0);
}

/// A node one window's document made, moved to another window's
/// document, takes its wrapper along: the iframe it came from going
/// leaves it as script made it, listeners included.
#[test]
fn a_node_moved_out_of_an_iframe_keeps_its_wrapper_when_the_iframe_goes() {
    let (mut e, mut h) = page("<body>");
    let src = "window.log = []; var f = document.body.appendChild(document.createElement('iframe')); var d = f.contentDocument;
var g = document.body.appendChild(document.createElement('iframe')); var gd = g.contentDocument;
var p = d.createElement('p'); p.id = 'moved'; p.addEventListener('x', () => log.push('moved')); p.mark = 1;
var r = d.createElement('i'); r.addEventListener('x', () => log.push('boxed'));
window.box = document.createElement('div'); box.appendChild(r); r = null;
var q = d.body.appendChild(d.createElement('p')); q.addEventListener('x', () => log.push('stayed'));
var s = gd.body.appendChild(d.createElement('b')); s.addEventListener('x', () => log.push('sibling'));
var t = d.body.appendChild(document.createElement('u'));
document.body.appendChild(p); window.q = q; window.s = s; window.t = t; f.remove(); return '';";
    assert_eq!(eval(&mut e, &mut h, src), "");
    collect(&mut e, &mut h);
    let src = "function throws(fn) { try { fn(); return 'no'; } catch (x) { return x.name; } }
for (var n of [document.getElementById('moved'), box.firstChild, s]) n.dispatchEvent(new Event('x'));
var p = document.getElementById('moved');
return [log.join(), p.mark, p.ownerDocument === document, throws(() => q.dispatchEvent(new Event('x'))),
  t.textContent = 'top', t.isConnected, window.length].join();";
    assert_eq!(eval(&mut e, &mut h, src), "moved,boxed,sibling,1,true,TypeError,top,false,1");
    let src = "function throws(fn) { try { fn(); return 'no'; } catch (x) { return x.name; } }
document.querySelector('iframe').remove(); return throws(() => s.dispatchEvent(new Event('x')));";
    assert_eq!(eval(&mut e, &mut h, src), "TypeError");
    collect(&mut e, &mut h);
    assert_eq!(h.dom.audit().mismatches(), 0);
    h.close(&mut e);
}

/// Realm slots are used again: a page making and removing iframes in a
/// loop keeps a table of two, and what script kept of a window whose
/// slot another window has now still reaches nothing.
#[test]
fn a_thousand_iframes_made_and_removed_keep_the_realm_table_bounded() {
    let (mut e, mut h) = page("<body>");
    let src = "window.old = document.body.appendChild(document.createElement('iframe')).contentWindow; old.frameElement.remove();
for (var i = 0; i < 1000; i++) { var f = document.body.appendChild(document.createElement('iframe')); f.contentWindow.Node; f.remove(); }
window.kept = document.body.appendChild(document.createElement('iframe')).contentWindow;
function throws(fn) { try { fn(); return 'no'; } catch (x) { return x.name; } }
return [throws(() => old.Node), throws(() => old.document.body), typeof kept.Node, kept.Node !== Node].join();";
    assert_eq!(eval(&mut e, &mut h, src), "TypeError,TypeError,function,true");
    assert_eq!(e.realm_slots(), 2, "the top-level realm and one slot used again");
    assert_eq!(e.live_realms(), 2);
    collect(&mut e, &mut h);
    h.close(&mut e);
    assert_eq!(e.live_realms(), 0);
}

/// The initial `about:blank` a loaded document replaced stays its
/// window's: its nodes' wrappers, made before or after, are that
/// window's realm's, and go with the navigable, not to the parent.
#[test]
fn the_replaced_initial_document_s_nodes_go_with_the_iframe() {
    let (mut e, mut h) = page("<body>");
    let src = "var f = document.createElement('iframe'); f.srcdoc = '<p>loaded'; document.body.appendChild(f);
window.first = f.contentWindow; window.blank = f.contentDocument; window.before = blank.createElement('i'); return '';";
    eval(&mut e, &mut h, src);
    while h.run_tasks(&mut e, 100) != 0 {}
    let src = "window.after = blank.createElement('b'); var f = document.querySelector('iframe');
return [f.contentDocument !== blank, blank.defaultView, before instanceof first.HTMLElement, after instanceof first.HTMLElement,
  after instanceof HTMLElement].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,,true,true,false");
    let src = "function throws(fn) { try { fn(); return 'no'; } catch (x) { return x.name; } }
document.querySelector('iframe').remove();
return [throws(() => before.dispatchEvent(new Event('x'))), throws(() => after.dispatchEvent(new Event('x'))),
  throws(() => blank.createElement('u'))].join();";
    assert_eq!(eval(&mut e, &mut h, src), "TypeError,TypeError,TypeError");
    collect(&mut e, &mut h);
    assert_eq!(h.dom.audit().mismatches(), 0);
    assert_eq!(e.live_realms(), 1);
    h.close(&mut e);
}

/// A host microtask queued in a window's realm that went before it ran
/// stays unrun, though another window has that realm's slot by then;
/// one queued in the realm of the window whose `queueMicrotask` was
/// called, whichever realm a wrapper was last made in.
#[test]
fn a_microtask_of_a_window_that_went_does_not_run_in_the_window_after_it() {
    let (mut e, mut h) = page("<body>");
    let src = "window.log = []; var f = document.body.appendChild(document.createElement('iframe')); var w = f.contentWindow;
w.queueMicrotask(() => log.push('old')); var b = f.contentDocument.body; queueMicrotask(() => log.push('top')); f.remove();
w.queueMicrotask(() => log.push('gone'));
var g = document.body.appendChild(document.createElement('iframe')); g.contentWindow.queueMicrotask(() => log.push('new'));
g.contentDocument.body; queueMicrotask(() => log.push('last'));
return log.join();";
    assert_eq!(eval(&mut e, &mut h, src), "");
    assert_eq!(e.realm_slots(), 2, "the second iframe has the first one's slot");
    assert_eq!(eval(&mut e, &mut h, "return log.join();"), "top,new,last");
    h.close(&mut e);
}

/// The word of a wrapper whose realm was detached is refused, whether
/// or not that realm's slot holds another realm by then.
#[test]
fn a_wrapper_of_a_detached_realm_is_refused() {
    let (mut e, mut h) = page("<body>");
    let src = "var d = document.body.appendChild(document.createElement('iframe')).contentDocument; window.kept = [d.body, d.head]; return '';";
    eval(&mut e, &mut h, src);
    let url = DomString::from_wtf8(b"iframe-lifetime");
    let realm = h.realm;
    let mut words = Vec::new();
    for name in ["kept[0]", "kept[1]"] {
        let v = e.evaluate(&mut h, realm, &DomString::from_wtf8(name.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
        words.push(Jsc::wrapper_word(v.get()));
        e.release(v);
    }
    assert_eq!(eval(&mut e, &mut h, "document.querySelector('iframe').remove(); return typeof kept[0];"), "object");
    assert!(e.wrapper_upgrade(words[0]).is_none(), "refused while the slot is free");
    let src = "var g = document.body.appendChild(document.createElement('iframe')); return g.contentDocument.body.localName;";
    assert_eq!(eval(&mut e, &mut h, src), "body");
    assert_eq!(e.realm_slots(), 2);
    assert!(e.wrapper_upgrade(words[1]).is_none(), "refused once another realm has the slot");
    h.close(&mut e);
}
