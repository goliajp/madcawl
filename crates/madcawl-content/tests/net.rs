//! Documents loaded over the network with their scripts run: script
//! sources come over the page's own connection, so in the session's
//! cookie partition and in its request log, and run in the standard's
//! order before the agent reads the page. The site is a small server on
//! 127.0.0.1.

mod common;

use std::net::TcpStream;
use std::time::{Duration, Instant};

use common::{Host, JS, executable, open_at, respond, site};
use madcawl_mcp::json;

const PAGE: &str = r#"<!doctype html><title>Scripts</title><body><p id=out>log:</p>
<script>
function log(s) { document.getElementById('out').appendChild(document.createTextNode('|' + s)); }
document.addEventListener('DOMContentLoaded', () => log('dcl'));
addEventListener('load', () => log('load'));
document.addEventListener('error', e => log('error:' + e.target.getAttribute('src')), true);
</script>
<script src=/blocking.js></script>
<script async src=/slow.js></script>
<script defer src=/defer.js></script>
<script src=/missing.js></script>
<script>log('inline:' + document.currentScript.localName)</script>
</body>"#;

/// A page that never loads: each script's fetch fails at once, and its
/// `error` handler adds another such script.
const ENDLESS: &str = r#"<!doctype html><title>Endless</title><script>
function f() { var s = document.createElement('script'); s.src = 'about:blank';
s.onerror = () => { s.remove(); f(); }; document.querySelector('head').appendChild(s); }
f();
</script>"#;

/// A page whose script reads its URL, asks to navigate away and throws.
const ERRORS: &str = r#"<!doctype html><title>Errors</title><p id=out></p><script>
document.getElementById('out').textContent = location.pathname + location.search + '|' + atob(btoa('ok'));
location.assign('/elsewhere?token=t1');
throw new TypeError('boom at ' + location.hostname);
</script>"#;

fn route(s: &mut TcpStream, path: &str, cookie: &str) -> bool {
    match path {
        "/" => respond(s, "200 OK", "Content-Type: text/html; charset=utf-8\r\nSet-Cookie: sid=k1; Path=/\r\n", PAGE.as_bytes()),
        "/blocking.js" if cookie.contains("sid=k1") => respond(s, "200 OK", JS, b"log('blocking:' + document.currentScript.getAttribute('src'))"),
        "/blocking.js" => respond(s, "200 OK", JS, b"log('blocking-without-the-cookie')"),
        "/spin" => respond(s, "200 OK", "Content-Type: text/html\r\n", b"<p>spin</p><script>while (true) {}</script>"),
        "/endless" => respond(s, "200 OK", "Content-Type: text/html\r\n", ENDLESS.as_bytes()),
        "/moved" => respond(s, "302 Found", "Location: /errors?from=moved\r\n", b""),
        "/errors?from=moved" => respond(s, "200 OK", "Content-Type: text/html\r\n", ERRORS.as_bytes()),
        "/defer.js" => respond(s, "200 OK", JS, b"log('defer')"),
        "/slow.js" => {
            std::thread::sleep(Duration::from_millis(300));
            respond(s, "200 OK", JS, b"log('async')");
        }
        _ => return false,
    }
    true
}

#[test]
fn a_page_s_scripts_come_over_its_connection_and_run_in_order_before_it_is_read() {
    let origin = site(route);
    let mut h = Host::start();
    let opened = h.call("open", &format!("{{\"path\":{}}}", json::s(&format!("{origin}/")).to_json()));
    let page = opened.split_whitespace().nth(1).unwrap().to_owned();
    let text = h.call("read", &format!(r#"{{"page":"{page}"}}"#));
    let want = "log:|blocking:/blocking.js|error:/missing.js|inline:script|defer|dcl|async|load";
    assert!(text.contains(want), "{text}");
    let log = h.call("requests", &format!(r#"{{"page":"{page}"}}"#));
    for js in ["/blocking.js", "/slow.js", "/defer.js", "/missing.js"] {
        assert!(log.lines().any(|l| l.contains("script") && l.contains(js)), "{js} in the request log: {log}");
    }
}

/// The page's URL is where the redirect led; what its script threw and
/// the navigation it asked for show in the page's request log.
#[test]
fn a_page_s_uncaught_exceptions_and_unfollowed_navigations_show_in_its_request_log() {
    let origin = site(route);
    let mut h = Host::start();
    let opened = h.call("open", &format!("{{\"path\":{}}}", json::s(&format!("{origin}/moved")).to_json()));
    let page = opened.split_whitespace().nth(1).unwrap().to_owned();
    let text = h.call("read", &format!(r#"{{"page":"{page}"}}"#));
    assert!(text.contains("/errors?from=moved|ok"), "{text}");
    let log = h.call("requests", &format!(r#"{{"page":"{page}"}}"#));
    assert!(log.contains("script: uncaught exception: TypeError: boom at 127.0.0.1 @inline:4:"), "{log}");
    // the document is still loading, so the navigation would replace it
    assert!(log.contains(&format!("script: navigation not followed: replace {origin}/elsewhere?token=t1")), "{log}");
}

#[test]
fn a_page_whose_scripts_never_let_it_load_still_returns_each_call_in_time() {
    let origin = site(route);
    let mut h = Host::start();
    let opened = h.call("open", &format!("{{\"path\":{}}}", json::s(&format!("{origin}/")).to_json()));
    let page = opened.split_whitespace().nth(1).unwrap().to_owned();
    let to = json::s(&format!("{origin}/endless")).to_json();
    let t0 = Instant::now();
    let acted = h.call("act", &format!(r#"{{"page":"{page}","action":"navigate","value":{to},"timeout_ms":1500}}"#));
    assert!(acted.contains("not stable after 1500 ms") && acted.contains("its window has not loaded"), "{acted}");
    let waited = h.call("wait", &format!(r#"{{"page":"{page}","timeout_ms":500}}"#));
    assert!(waited.contains("its window has not loaded"), "{waited}");
    assert!(t0.elapsed() < Duration::from_secs(10), "two calls of 1.5 s and 0.5 s took {:?}", t0.elapsed());
}

/// The executable, which isolates without being asked: the page lives in
/// a content process (this executable again), which fetches its scripts
/// the same way.
#[test]
fn the_executable_runs_a_page_s_scripts_in_its_content_process() {
    let origin = site(route);
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mcc-exe-{}", std::process::id()));
    let (_child, mut h) = executable(&dir, &[]);
    let (opened, _) = open_at(&mut h, &format!("{origin}/"));
    let page = opened.split_whitespace().nth(1).unwrap().to_owned();
    let text = h.call("read", &format!(r#"{{"page":"{page}"}}"#));
    assert!(text.contains("log:|blocking:/blocking.js|error:/missing.js|inline:script|defer|dcl|async|load"), "{text}");
}

/// A script that never returns takes its session's content process, which
/// the host kills; another session on the same host is answered meanwhile.
#[test]
fn a_script_that_never_returns_ends_its_content_process_and_not_the_host() {
    let origin = site(route);
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mcc-spin-{}", std::process::id()));
    let (_child, mut spun) = executable(&dir, &["--hang-secs", "2", "--script-ms", "0"]);
    let mut other = Host { dir: dir.join("other"), mcp: madcawl_mcp::server::Server::new(dir.join("s"), dir.clone()) };
    let t0 = Instant::now();
    let (text, error) = open_at(&mut spun, &format!("{origin}/spin"));
    assert!(error && text.contains("did not answer within 2 s"), "{text}");
    let (opened, error) = open_at(&mut other, &format!("{origin}/"));
    assert!(!error, "{opened}");
    let page = opened.split_whitespace().nth(1).unwrap().to_owned();
    let text = other.call("read", &format!(r#"{{"page":"{page}"}}"#));
    assert!(text.contains("|load"), "{text}");
    assert!(t0.elapsed() < Duration::from_secs(20), "{:?}", t0.elapsed());
}
