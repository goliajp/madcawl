//! XMLHttpRequest and the window it was made in: a request names that
//! window's document as its initiator, its timeout timer goes with the
//! window, and it keeps one timer however often its timeout changes.

use madcawl_bindings::{DomHost, FetchResponse};
use madcawl_dom::Dom;
use madcawl_js::{Engine, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

fn page(virtual_time: bool) -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    assert!(h.set_url("https://a.test/dir/page?q=1"));
    h.install_window(&mut e);
    if virtual_time {
        h.use_virtual_time();
    }
    h.parse_document(&mut e, b"<body>");
    run(&mut e, &mut h);
    (e, h)
}

fn run(e: &mut Jsc, h: &mut DomHost<Jsc>) {
    while h.run_tasks(e, 100) != 0 {}
}

/// The result of `src` as a string, or what it threw.
fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"xhr");
    let realm = h.realm;
    let src = format!("String((function () {{ try {{ {src} }} catch (x) {{ return 'threw ' + x.name; }} }})())");
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    assert!(!Jsc::is_thrown(r.get()), "{src} threw");
    let mut b = Vec::new();
    e.to_string(h, r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    h.settle(e);
    String::from_utf8(b).unwrap()
}

const SEND: &str = "window.send = (w, url) => { const x = new w.XMLHttpRequest(); x.open('GET', url); x.send(); return x; };";

#[test]
fn a_request_is_initiated_by_the_document_of_its_window() {
    let (mut e, mut h) = page(true);
    eval(&mut e, &mut h, SEND);
    let src = "const blank = document.createElement('iframe'); document.body.appendChild(blank);
window.child = document.createElement('iframe'); child.src = 'child'; document.body.appendChild(child);
send(window, 'top'); send(blank.contentWindow, 'blank'); return '';";
    eval(&mut e, &mut h, src);
    let (id, url) = h.take_frame_requests().remove(0);
    assert_eq!(url, "https://a.test/dir/child");
    h.frame_fetched(id, Some(("https://a.test/dir/child?x=1", b"<body>")));
    run(&mut e, &mut h);
    eval(&mut e, &mut h, "send(child.contentWindow, 'framed');");
    let got: Vec<(String, Option<String>)> = h.take_fetches().into_iter().map(|r| (r.url, r.initiator)).collect();
    let want = [("https://a.test/dir/top", "https://a.test/dir/page?q=1"), ("https://a.test/dir/blank", "https://a.test/dir/page?q=1"), ("https://a.test/dir/framed", "https://a.test/dir/child?x=1")];
    assert_eq!(got, want.map(|(u, i)| (u.to_owned(), Some(i.to_owned()))), "about:blank takes its parent's");
    h.close(&mut e);
}

#[test]
fn a_timeout_pending_in_a_removed_iframe_goes_with_it() {
    let (mut e, mut h) = page(true);
    eval(&mut e, &mut h, SEND);
    let src = "const f = document.createElement('iframe'); document.body.appendChild(f); window.log = [];
window.x = send(f.contentWindow, '/slow'); x.timeout = 10; x.ontimeout = () => log.push('timeout');
window.sig = f.contentWindow.AbortSignal.timeout(10); sig.onabort = () => log.push('abort');
const keep = new XMLHttpRequest(); keep.open('GET', '/kept'); keep.timeout = 20; keep.ontimeout = () => log.push('kept timeout'); keep.send();
f.remove(); return '';";
    eval(&mut e, &mut h, src);
    let reqs = h.take_fetches();
    assert_eq!(reqs.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(), ["https://a.test/kept"], "the iframe's request is not asked for");
    assert_eq!((h.fetches_in_flight(), h.event_loop().pending()), (1, 1), "the top window's request and its one timer are left");
    h.fetch_done(reqs[0].id, None);
    run(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return log.join();"), "");
    assert_eq!(h.event_loop().pending(), 0, "an ended request's timer goes with it");
    h.close(&mut e);
}

#[test]
fn a_request_keeps_one_timer_however_often_its_timeout_changes() {
    let (mut e, mut h) = page(false);
    eval(&mut e, &mut h, SEND);
    eval(&mut e, &mut h, "window.log = []; window.x = send(window, '/slow'); x.ontimeout = () => log.push('timeout ' + x.readyState);");
    let id = h.take_fetches()[0].id;
    eval(&mut e, &mut h, "for (let i = 0; i < 1000; i++) x.timeout = 100000 + i;");
    assert_eq!(h.event_loop().pending(), 1);
    eval(&mut e, &mut h, "x.timeout = 0;");
    assert_eq!(h.event_loop().pending(), 0);
    eval(&mut e, &mut h, "x.timeout = 100000; x.timeout = 5;");
    assert_eq!(h.event_loop().pending(), 1);
    std::thread::sleep(std::time::Duration::from_millis(20));
    run(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return log.join();"), "timeout 4");
    assert_eq!((h.event_loop().pending(), h.fetches_in_flight()), (0, 0));
    h.fetch_done(id, Some(FetchResponse { status: 200, status_text: "OK".into(), url: "https://a.test/slow".into(), headers: Vec::new(), body: b"late".to_vec() }));
    run(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return x.status;"), "0");
    h.close(&mut e);
}
