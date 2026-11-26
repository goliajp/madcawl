//! What an event holds on JavaScriptCore: its target's tree lives while
//! script reaches the event, a cycle through script is still freed, the
//! work of tree changes does not grow with the events kept nor that of
//! a dispatch with its target's tree, a dispatch keeps the trees its
//! path started in, and closing the page leaves the wrappers' member
//! containers alone.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Bound, Ctx, Deadline, Engine, Key, ScriptOrigin, Step};
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

/// The wrapper words placed by 300 insert and remove pairs with `kept`
/// events kept, each dispatched on a detached div of its own.
fn tree_changes_with(kept: usize) -> u64 {
    let (mut e, mut h) = page("<p>x</p>");
    let src = format!("var kept = []; for (let i = 0; i < {kept}; i++) {{ let ev = new Event('x');
document.createElement('div').dispatchEvent(ev); kept.push(ev); }}
var b = document.querySelector('body'), c = document.createElement('span'); 'ok'");
    assert_eq!(eval(&mut e, &mut h, &src), "ok");
    let before = e.words_placed();
    eval(&mut e, &mut h, "for (let i = 0; i < 300; i++) { b.appendChild(c); b.removeChild(c); } 'ok'");
    let placed = e.words_placed() - before;
    h.close(&mut e);
    placed
}

#[test]
fn tree_changes_place_the_same_however_many_events_are_kept() {
    let few = tree_changes_with(100);
    assert!(few > 0, "each move places the moved wrapper");
    assert_eq!(tree_changes_with(1600), few);
}

/// The wrapper words placed by 200 dispatches on a node of a detached
/// tree of `size` wrapped nodes, with a listener on the target and one
/// on the root.
fn dispatches_in_tree_of(size: usize) -> u64 {
    let (mut e, mut h) = page("<p>x</p>");
    let src = format!("var root = document.createElement('div'), ws = [root], seen = 0;
for (let i = 1; i < {size}; i++) {{ let n = document.createElement('i'); n.x = i; root.appendChild(n); ws.push(n); }}
var t = root.firstChild; t.addEventListener('x', () => seen++); root.addEventListener('x', () => seen++, true); 'ok'");
    assert_eq!(eval(&mut e, &mut h, &src), "ok");
    let before = e.words_placed();
    assert_eq!(eval(&mut e, &mut h, "for (let i = 0; i < 200; i++) t.dispatchEvent(new Event('x')); String(seen)"), "400");
    let placed = e.words_placed() - before;
    h.close(&mut e);
    placed
}

#[test]
fn a_dispatch_works_the_same_however_large_its_target_tree() {
    assert_eq!(dispatches_in_tree_of(5000), dispatches_in_tree_of(10));
}

/// `gc()`: a full collection from inside a script.
unsafe extern "C" fn gc_fn(ctx: *mut Ctx, _: u64, _: *const u64, _: usize) -> u64 {
    // SAFETY: bound to Jsc and DomHost<Jsc>
    let (e, _) = unsafe { Ctx::parts::<Jsc, DomHost<Jsc>>(ctx) };
    scrub();
    e.collect_now();
    Jsc::to_bits(Jsc::undefined())
}

#[test]
fn a_dispatch_keeps_the_tree_a_listener_moved_its_path_out_of() {
    let (mut e, mut h) = page("<p>x</p>");
    // SAFETY: gc_fn reads its context as Jsc and DomHost<Jsc>
    let f = e.new_function(h.realm, "gc", 0, unsafe { Bound::<_, Jsc, DomHost<Jsc>>::host_fn(gc_fn) }, 0);
    let g = e.global(h.realm);
    assert!(e.set(&mut h, g.get(), Key::Str(&DomString::from_wtf8(b"gc")), f.get()));
    e.release(g);
    e.release(f);
    // script reaches the target only; the root, its expando and its
    // listener only through the target's tree, until a listener moves
    // the target's parent into another tree and the root's wrapper is
    // left to the dispatch
    let src = "var t, got = 'none'; function onRoot() { got = this.x; }
function onTarget() { var n = document.createElement('p'); n.appendChild(t.parentNode); n = null; gc(); }
(function () { var r = document.createElement('div'); r.x = 'kept'; r.addEventListener('x', onRoot);
t = r.appendChild(document.createElement('span')).appendChild(document.createElement('i')); t.addEventListener('x', onTarget); })(); 'ok'";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    collect_everything(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "t.dispatchEvent(new Event('x', { bubbles: true })); got"), "kept");
    assert_eq!(h.dom.audit().mismatches(), 0);
    h.close(&mut e);
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
