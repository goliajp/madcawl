//! Pages that once broke the adapter or the host around it: timers that
//! clear themselves or wait too long to represent, script that moves
//! nodes the parser holds, a stop asked for under a host call, writes
//! nested without end, and an isolate that outlives its host.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Control, Deadline, Engine, Interrupt, ScriptOrigin, Step};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

/// Ends the test process when a case hangs, which is how these failed.
fn guard(secs: u64, what: &'static str) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(secs));
        eprintln!("hung: {what}");
        std::process::exit(3);
    });
}

fn page(html: &str, virtual_time: bool) -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    if virtual_time {
        h.use_virtual_time();
    }
    h.parse_document(&mut e, html.as_bytes());
    (e, h)
}

fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> f64 {
    let url = DomString::from_wtf8(b"regress");
    let realm = h.realm;
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    let n = e.as_number(r.get()).expect("a number");
    e.release(r);
    h.settle(e);
    n
}

fn dat(h: &DomHost<Jsc>) -> String {
    let mut out = Vec::new();
    h.dom.write_dat(h.dom.document(), &mut out);
    String::from_utf8(out).unwrap()
}

#[test]
fn an_interval_cleared_by_its_own_callback_stops() {
    guard(20, "interval");
    let (mut e, mut h) = page(
        "<script>var n = 0; var id = setInterval(function () { n++; if (n == 3) clearInterval(id); }, 1);</script>",
        true,
    );
    let ran = h.run_tasks(&mut e, 50);
    let pending = h.event_loop().pending();
    assert_eq!((ran, pending, eval(&mut e, &mut h, "n")), (3, 0, 3.0));
    h.close(&mut e);
}

#[test]
fn a_zero_interval_is_clamped_once_its_runs_nest_past_five() {
    let (mut e, mut h) = page("<script>setInterval(function () {}, 0);</script>", true);
    let mut waits = Vec::new();
    for _ in 0..7 {
        assert_eq!(h.run_tasks(&mut e, 1), 1);
        waits.push(h.event_loop().next_due_in().expect("the interval") / 1_000_000);
    }
    assert_eq!(waits, [0, 0, 0, 0, 4, 4, 4]);
    h.close(&mut e);
}

#[test]
fn a_timeout_past_a_long_wraps_as_web_idl_says() {
    guard(20, "huge timeout");
    // ToInt32(1e20) is 1661992960 ms, about 19 days: not due now
    let (mut e, mut h) = page("<script>var fired = 0; setTimeout(function () { fired = 1; }, 1e20);</script>", false);
    assert_eq!(h.run_tasks(&mut e, 5), 0);
    // 2^32 + 5 wraps to 5, and -1 clamps to 0: both due at once
    eval(&mut e, &mut h, "setTimeout(function () { fired += 10; }, 4294967301); setTimeout(function () { fired += 100; }, -1); 0");
    h.use_virtual_time();
    assert_eq!(h.run_tasks(&mut e, 2), 2);
    assert_eq!(eval(&mut e, &mut h, "fired"), 110.0);
    h.close(&mut e);
}

#[test]
fn the_parser_never_puts_a_node_under_itself_after_script_moved_it() {
    guard(20, "moved nodes");
    let html = "<body><div id=ca><a id=fe><p id=fb>x<script>\
var a = document.querySelector('#fe'), p = document.querySelector('#fb'), d = document.querySelector('#ca');\
document.querySelector('body').appendChild(a); p.appendChild(d);\
</script></a>y";
    let (mut e, h) = page(html, true);
    let d = dat(&h);
    assert_eq!(h.dom.audit().mismatches(), 0);
    assert_eq!(d.matches("id=\"ca\"").count(), 1, "{d}");
    h.close(&mut e);
}

#[test]
fn a_termination_ends_script_that_catches_under_a_host_call() {
    guard(20, "terminate under a host call");
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    let i = e.interrupt_handle();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        i.terminate();
    });
    h.parse_document(
        &mut e,
        b"<script>for (;;) { try { document.write('<script>for (;;) {}<\\/script>'); } catch (x) {} }</script>",
    );
    assert_eq!(e.halt(), madcawl_js::Halt::Terminated);
    h.close(&mut e);
}

#[test]
fn an_abort_ends_the_outer_script_too_and_the_isolate_resumes() {
    guard(20, "abort under a host call");
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    let i = e.interrupt_handle();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        i.abort().unwrap();
    });
    h.parse_document(
        &mut e,
        b"<script>for (;;) { try { document.write('<script>for (;;) {}<\\/script>'); } catch (x) {} }</script>",
    );
    assert_eq!(e.halt(), madcawl_js::Halt::Aborted);
    assert_eq!(e.resume(), Ok(madcawl_js::Halt::Running));
    assert_eq!(eval(&mut e, &mut h, "6 * 7"), 42.0);
    h.close(&mut e);
}

#[test]
fn document_write_nested_past_21_is_ignored() {
    guard(60, "nested writes");
    let (mut e, mut h) = page(
        "<script>var depth = 0; function f() { depth++; document.write('<script>f()<\\/script>'); } f();</script>",
        true,
    );
    assert_eq!(h.reported, 0);
    // the first call is the page's script; each write nests one more
    assert_eq!(eval(&mut e, &mut h, "depth"), 22.0);
    h.close(&mut e);
}

#[test]
fn closing_lets_go_of_the_wrappers_of_detached_nodes() {
    guard(60, "detached wrappers");
    let (mut e, mut h) = page("<p>x</p>", true);
    let before = h.dom.node_count();
    eval(&mut e, &mut h, "for (let i = 0; i < 500; i++) { let d = document.createElement('div'); d.appendChild(document.createElement('span')); } 0");
    while h.collect(&mut e, Deadline::Work(u32::MAX)) != Step::Done {}
    // the bound until tree groups: script-made detached nodes live until
    // the host closes
    assert_eq!(h.dom.node_count(), before + 1000);
    assert_eq!(h.dom.wrapped().len(), 1000 + 1);
    h.close(&mut e);
}

#[test]
fn wrappers_that_die_after_their_host_closed_reach_no_other_host() {
    guard(60, "isolate outlives a host");
    let (mut e, h1) = page("<p>x</p><p>y</p><script>var all = document.querySelectorAll('p'); all.length;</script>", true);
    h1.close(&mut e);
    let mut h2 = DomHost::new(&mut e, Dom::new());
    h2.install_window(&mut e);
    h2.parse_document(&mut e, b"<div>z</div>");
    // allocate until the collector has run and finalized the first
    // host's wrappers; each notice would have reached this host
    let churn = "var keep = []; for (let i = 0; i < 300000; i++) { keep.push(new Array(16)); if (keep.length > 1000) keep = []; } 0";
    for _ in 0..5 {
        eval(&mut e, &mut h2, churn);
        assert_eq!(h2.dom.audit().mismatches(), 0);
    }
    h2.close(&mut e);
}

#[test]
fn query_selector_takes_braces_and_semicolons_in_strings() {
    let (mut e, mut h) = page("<p title='a{b}c;'>x</p><p>y</p>", true);
    assert_eq!(eval(&mut e, &mut h, "document.querySelectorAll('p[title=\"a{b}c;\"]').length"), 1.0);
    let threw = "var r; try { document.querySelector('p{}'); r = 0; } catch (x) { r = x.name == 'SyntaxError' ? 1 : 2; } r";
    assert_eq!(eval(&mut e, &mut h, threw), 1.0);
    h.close(&mut e);
}
