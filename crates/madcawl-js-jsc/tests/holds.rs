//! What an event holds on JavaScriptCore: its target's tree lives while
//! script reaches the event, a cycle through script is still freed, the
//! cost of tree changes does not grow with the events kept, and closing
//! the page leaves the wrappers' member containers alone.

use std::time::{Duration, Instant};

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
    let url = DomString::from_wtf8(b"holds");
    let realm = h.realm;
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
fn an_event_its_target_keeps_through_script_is_freed_with_it() {
    let (mut e, mut h) = page("<p>x</p>");
    let (nodes, sides) = (h.dom.node_count(), h.side_objects());
    let src = "for (let i = 0; i < 200; i++) (function () { var d = document.createElement('div'), last;
d.addEventListener('x', ev => { last = ev; }); d.dispatchEvent(new Event('x')); })(); 'ok'";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    collect_everything(&mut e, &mut h);
    assert_eq!(h.dom.node_count(), nodes);
    assert_eq!(h.side_objects(), sides);
    assert_eq!(h.dom.audit().mismatches(), 0);
    h.close(&mut e);
}

#[test]
fn an_event_kept_on_its_event_target_is_freed_with_it() {
    let (mut e, mut h) = page("<p>x</p>");
    let sides = h.side_objects();
    let src = "for (let i = 0; i < 200; i++) (function () { var t = new EventTarget();
t.addEventListener('x', ev => { t.last = ev; }); t.dispatchEvent(new Event('x')); })(); 'ok'";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    collect_everything(&mut e, &mut h);
    assert_eq!(h.side_objects(), sides);
    h.close(&mut e);
}

#[test]
fn an_event_still_keeps_its_target_tree_while_script_holds_it() {
    let (mut e, mut h) = page("<p>x</p>");
    let src = "var t = new EventTarget(); t.y = 'own'; var evs = [new Event('x'), new Event('x')];
(function () { var d = document.createElement('div'); d.appendChild(document.createElement('i')); d.firstChild.x = 'kept';
d.firstChild.dispatchEvent(evs[0]); })(); t.dispatchEvent(evs[1]); t = null; 'ok'";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    collect_everything(&mut e, &mut h);
    // moved into another detached tree after the dispatch, still kept
    let src = "var i = evs[0].target; var p = document.createElement('p'); p.appendChild(i); i = p = null; 'ok'";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    collect_everything(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "[evs[0].target.x, evs[0].target.parentNode.nodeName, evs[1].target.y].join()"), "kept,P,own");
    h.close(&mut e);
}

/// The time 300 insert and remove pairs take with `kept` events kept,
/// each dispatched on a detached div of its own; the best of three.
fn tree_changes_with(kept: usize) -> Duration {
    let (mut e, mut h) = page("<p>x</p>");
    let src = format!("var kept = []; for (let i = 0; i < {kept}; i++) {{ let ev = new Event('x');
document.createElement('div').dispatchEvent(ev); kept.push(ev); }}
var b = document.querySelector('body'), c = document.createElement('span'); 'ok'");
    assert_eq!(eval(&mut e, &mut h, &src), "ok");
    let best = (0..3)
        .map(|_| {
            let t = Instant::now();
            eval(&mut e, &mut h, "for (let i = 0; i < 300; i++) { b.appendChild(c); b.removeChild(c); } 'ok'");
            t.elapsed()
        })
        .min()
        .expect("three runs");
    h.close(&mut e);
    best
}

#[test]
fn tree_changes_cost_the_same_however_many_events_are_kept() {
    let (few, many) = (tree_changes_with(100), tree_changes_with(1600));
    // linear in the events kept was 16 times; allow noise, not growth
    assert!(many < few * 4 + Duration::from_millis(20), "100 kept: {few:?}, 1600 kept: {many:?}");
}

#[test]
fn closing_leaves_the_member_containers_of_weak_wrappers_alone() {
    let (mut e, mut h) = page("<p>x</p>");
    // members of detached wrappers, one script still reaches and one it
    // dropped, and the detail of a dropped event
    let src = "var kept = document.createElement('div'); kept.addEventListener('x', () => 1);
(function () { var d = document.createElement('div'); d.addEventListener('x', () => d); new CustomEvent('c', { detail: {} }); })(); 'ok'";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    let before = e.member_writes();
    h.close(&mut e);
    assert_eq!(e.member_writes(), before, "close forgets members instead of writing into their containers");
}

#[inline(never)]
fn scrub() {
    let pad = [0u8; 512 * 1024];
    std::hint::black_box(&pad);
}
