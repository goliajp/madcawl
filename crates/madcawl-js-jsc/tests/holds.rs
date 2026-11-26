//! What the host holds on JavaScriptCore: closing the page leaves the
//! wrappers' member containers alone.

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
