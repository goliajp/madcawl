//! `document.readyState` with its `readystatechange` events, `title` and
//! `defaultView`, on JavaScriptCore.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Control, Engine, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

/// A loaded page of `html`, and the result of `src` run after it as a
/// string.
fn run(html: &str, src: &str) -> String {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, html.as_bytes());
    while h.run_tasks(&mut e, 100) != 0 {}
    assert!(h.is_loaded());
    let url = DomString::from_wtf8(b"state");
    let realm = h.realm;
    let src = format!("String((function () {{ {src} }})())");
    let r = e.evaluate(&mut h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    assert!(!Jsc::is_thrown(r.get()));
    let mut b = Vec::new();
    e.to_string(&mut h, r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    h.close(&mut e);
    String::from_utf8(b).unwrap()
}

#[test]
fn ready_state_moves_with_loading_and_says_so() {
    let html = "<script>var log = [document.readyState];
document.addEventListener('readystatechange', () => log.push('change:' + document.readyState));
document.addEventListener('DOMContentLoaded', () => log.push('dcl:' + document.readyState));
window.addEventListener('load', () => log.push('load:' + document.readyState));</script><p>x";
    assert_eq!(run(html, "return log.join();"), "loading,change:interactive,dcl:interactive,change:complete,load:complete");
}

#[test]
fn title_reads_and_writes_the_title_element() {
    let html = "<head><title>  a \n b\t</title></head><body>";
    let src = "var out = [document.title]; document.title = ' x  y '; out.push(document.title, document.querySelector('title').textContent);
return out.join('|');";
    assert_eq!(run(html, src), "a b|x y| x  y ");
    let src = "var out = [JSON.stringify(document.title)]; document.title = 't';
out.push(document.title, document.head.lastChild.localName, document.querySelectorAll('title').length);
var t = document.querySelector('title'); t.parentNode.removeChild(t); document.documentElement.removeChild(document.head); document.title = 'u'; out.push(JSON.stringify(document.title));
return out.join('|');";
    assert_eq!(run("<head><meta></head><body>", src), "\"\"|t|title|1|\"\"");
}

#[test]
fn default_view_is_the_window() {
    assert_eq!(run("<p>", "return [document.defaultView === window, document.defaultView === globalThis].join();"), "true,true");
}

#[test]
fn a_ready_state_listener_stopped_by_the_time_limit_does_not_stop_loading() {
    let mut e = Jsc::new();
    e.set_time_limit(Some(std::time::Duration::from_millis(200))).unwrap();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    h.use_virtual_time();
    let html = "<script>var log = [];
document.addEventListener('readystatechange', () => { log.push('rs:' + document.readyState); if (document.readyState == 'interactive') for (;;) {} });
document.addEventListener('DOMContentLoaded', () => log.push('dcl'));
window.addEventListener('load', () => log.push('load'));</script><script defer src=d.js></script><p>x";
    h.parse_document(&mut e, html.as_bytes());
    for _ in 0..100 {
        for (id, _) in h.take_script_requests() {
            h.script_fetched(id, Some(DomString::from_wtf8(b"log.push('defer')")));
        }
        if h.run_tasks(&mut e, 100) == 0 && h.is_loaded() {
            break;
        }
    }
    assert!(h.is_loaded());
    let notes: Vec<String> = h.take_notes().iter().map(ToString::to_string).collect();
    assert_eq!(notes, ["uncaught exception: script stopped: it ran past the time limit"]);
    let url = DomString::from_wtf8(b"state");
    let realm = h.realm;
    let r = e.evaluate(&mut h, realm, &DomString::from_wtf8(b"log.join()"), &madcawl_js::ScriptOrigin { url: &url, line: 1, column: 0 });
    let mut b = Vec::new();
    e.as_string(r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    assert_eq!(String::from_utf8(b).unwrap(), "rs:interactive,defer,dcl,rs:complete,load");
    h.close(&mut e);
}
