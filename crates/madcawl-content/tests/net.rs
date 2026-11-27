//! Documents loaded over the network with their scripts run: script
//! sources come over the page's own connection, so in the session's
//! cookie partition and in its request log, and run in the standard's
//! order before the agent reads the page. The site is a small server on
//! 127.0.0.1.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use madcawl_mcp::json::{self, Value};
use madcawl_shell::serve::{Config, Server};

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

fn respond(s: &mut TcpStream, status: &str, extra: &str, body: &[u8]) {
    let head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n{extra}Content-Length: {}\r\n\r\n", body.len());
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

fn handle(mut s: TcpStream) {
    let mut buf = Vec::new();
    let mut b = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match s.read(&mut b) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&b[..n]),
        }
    }
    let req = String::from_utf8_lossy(&buf).to_ascii_lowercase();
    let path = req.split_whitespace().nth(1).unwrap_or("").to_owned();
    let cookie = req.lines().find_map(|l| l.strip_prefix("cookie:")).unwrap_or("").to_owned();
    const JS: &str = "Content-Type: text/javascript\r\n";
    match path.as_str() {
        "/" => respond(&mut s, "200 OK", "Content-Type: text/html; charset=utf-8\r\nSet-Cookie: sid=k1; Path=/\r\n", PAGE.as_bytes()),
        "/blocking.js" if cookie.contains("sid=k1") => respond(&mut s, "200 OK", JS, b"log('blocking:' + document.currentScript.getAttribute('src'))"),
        "/blocking.js" => respond(&mut s, "200 OK", JS, b"log('blocking-without-the-cookie')"),
        "/spin" => respond(&mut s, "200 OK", "Content-Type: text/html\r\n", b"<p>spin</p><script>while (true) {}</script>"),
        "/endless" => respond(&mut s, "200 OK", "Content-Type: text/html\r\n", ENDLESS.as_bytes()),
        "/moved" => respond(&mut s, "302 Found", "Location: /errors?from=moved\r\n", b""),
        "/errors?from=moved" => respond(&mut s, "200 OK", "Content-Type: text/html\r\n", ERRORS.as_bytes()),
        "/defer.js" => respond(&mut s, "200 OK", JS, b"log('defer')"),
        "/slow.js" => {
            std::thread::sleep(Duration::from_millis(300));
            respond(&mut s, "200 OK", JS, b"log('async')");
        }
        _ => respond(&mut s, "404 Not Found", JS, b"log('a 404 body ran')"),
    }
}

fn site() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || handle(s));
        }
    });
    format!("http://{addr}")
}

struct Host {
    dir: PathBuf,
    mcp: madcawl_mcp::server::Server,
}

impl Host {
    fn start() -> Host {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mcc-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        madcawl_shell::serve::scripts::set_scripts(madcawl_content::open);
        let mut config = Config::new(dir.join("s"));
        config.allow = vec![dir.clone()];
        config.net = true;
        let server = Server::bind(config.clone()).unwrap();
        std::thread::spawn(move || server.run());
        let mcp = madcawl_mcp::server::Server::new(config.socket.clone(), dir.clone());
        Host { dir, mcp }
    }

    /// The reply's text, and whether it is an error.
    fn call_any(&mut self, tool: &str, args: &str) -> (String, bool) {
        let line = format!(r#"{{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{{"name":"{tool}","arguments":{args}}}}}"#);
        let v = self.mcp.handle(&line).unwrap();
        let r = v.get("result").unwrap();
        let Some(Value::Arr(c)) = r.get("content") else { panic!("{}", v.to_json()) };
        (c[0].str_at("text").unwrap().to_owned(), r.bool_at("isError").unwrap())
    }

    fn call(&mut self, tool: &str, args: &str) -> String {
        let (text, error) = self.call_any(tool, args);
        assert!(!error, "{tool} {args}: {text}");
        text
    }
}

/// A host process, ended when the test ends, a failed one too.
struct Child(std::process::Child);

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_page_s_scripts_come_over_its_connection_and_run_in_order_before_it_is_read() {
    let origin = site();
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
    let origin = site();
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
    let origin = site();
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

/// The executable, listening on a socket in `dir`, and an agent session
/// on it.
fn executable(dir: &std::path::Path, extra: &[&str]) -> (Child, Host) {
    std::fs::create_dir_all(dir).unwrap();
    let sock = dir.join("s");
    let child = Child(
        std::process::Command::new(env!("CARGO_BIN_EXE_madcawl-content"))
            .args(["--serve", sock.to_str().unwrap(), "--net", "--exit-idle", "30"])
            .args(extra)
            .spawn()
            .unwrap(),
    );
    let t0 = Instant::now();
    while !sock.exists() {
        assert!(t0.elapsed() < Duration::from_secs(10), "the host never listened");
        std::thread::sleep(Duration::from_millis(20));
    }
    (child, Host { dir: dir.to_owned(), mcp: madcawl_mcp::server::Server::new(sock, dir.to_owned()) })
}

fn open_at(h: &mut Host, url: &str) -> (String, bool) {
    h.call_any("open", &format!("{{\"path\":{}}}", json::s(url).to_json()))
}

/// The executable, which isolates without being asked: the page lives in
/// a content process (this executable again), which fetches its scripts
/// the same way.
#[test]
fn the_executable_runs_a_page_s_scripts_in_its_content_process() {
    let origin = site();
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
    let origin = site();
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mcc-spin-{}", std::process::id()));
    let (_child, mut spun) = executable(&dir, &["--hang-secs", "2"]);
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
