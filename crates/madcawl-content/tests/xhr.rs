//! XMLHttpRequest of documents loaded over the network: requests that
//! go through the network service with their method, headers, body and
//! the session's cookies, a CORS response read only when it allows it,
//! and each in the page's request log; in the host's own process and in
//! a content process. The sites are small servers on 127.0.0.1.

mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use common::{Host, executable, open_at};

/// A request as the site saw it: method, path, lowercased headers, body.
struct Req {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Req {
    fn header(&self, name: &str) -> &str {
        self.headers.iter().find(|(n, _)| n == name).map_or("", |(_, v)| v.as_str())
    }
}

fn read_req(s: &mut TcpStream) -> Option<Req> {
    let mut buf = Vec::new();
    let mut b = [0u8; 4096];
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        let n = s.read(&mut b).ok().filter(|&n| n > 0)?;
        buf.extend_from_slice(&b[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let (method, path) = (first.next()?.to_owned(), first.next()?.to_owned());
    let headers: Vec<(String, String)> =
        lines.filter_map(|l| l.split_once(':')).map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned())).collect();
    let len: usize = headers.iter().find(|(n, _)| n == "content-length").map_or(0, |(_, v)| v.parse().unwrap_or(0));
    let mut body = buf[end..].to_vec();
    while body.len() < len {
        let n = s.read(&mut b).ok().filter(|&n| n > 0)?;
        body.extend_from_slice(&b[..n]);
    }
    Some(Req { method, path, headers, body })
}

fn respond(s: &mut TcpStream, status: &str, extra: &str, body: &[u8]) {
    let head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n{extra}Content-Length: {}\r\n\r\n", body.len());
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

/// A site on 127.0.0.1 answering with `f`; its origin.
fn serve(f: impl Fn(&Req, &mut TcpStream) + Send + Sync + 'static) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let f = Arc::new(f);
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let f = f.clone();
            std::thread::spawn(move || {
                if let Some(r) = read_req(&mut s) {
                    f(&r, &mut s);
                }
            });
        }
    });
    format!("http://{addr}")
}

const PAGE: &str = r#"<!doctype html><title>Xhr</title><p id=out></p><script>
function log(s) { document.getElementById('out').appendChild(document.createTextNode('|' + s)); }
var x = new XMLHttpRequest(); x.open('POST', '/echo?a=1'); x.setRequestHeader('X-Test', 't1'); x.setRequestHeader('Content-Type', 'text/plain');
x.onload = function () { log(x.status + ' ' + x.statusText + ' ' + x.getResponseHeader('x-echo') + ' ' + x.responseText + ' jar=' + document.cookie); };
x.onerror = function () { log('echo error'); }; x.send('body-1');
var y = new XMLHttpRequest(); y.open('GET', OTHER + '/open');
y.onload = function () { log('open ' + y.responseText + ' ' + y.getAllResponseHeaders().indexOf('x-secret')); };
y.onerror = function () { log('open error'); }; y.send();
var z = new XMLHttpRequest(); z.open('GET', OTHER + '/closed');
z.onload = function () { log('closed read ' + z.responseText); }; z.onerror = function () { log('closed error ' + z.status); }; z.send();
var v = new XMLHttpRequest(); v.open('GET', '/hop'); v.onload = function () { log('hop ' + v.responseText); }; v.onerror = function () { log('hop error'); }; v.send();
</script><script src=/wait.js></script>"#;

/// The page's site, and the other origin's, which counts its answers.
fn sites() -> String {
    let served = Arc::new(AtomicUsize::new(0));
    let other_served = served.clone();
    let other = serve(move |r, s| {
        match r.path.as_str() {
            "/open" => {
                let text = format!("cookie={} origin={}", r.header("cookie"), r.header("origin"));
                respond(s, "200 OK", "Access-Control-Allow-Origin: *\r\nX-Secret: 1\r\n", text.as_bytes());
            }
            _ => respond(s, "200 OK", "", b"secret"),
        }
        other_served.fetch_add(1, Ordering::SeqCst);
    });
    let hop = format!("Location: {other}/open\r\n");
    serve(move |r, s| match r.path.as_str() {
        "/hop" => respond(s, "302 Found", &hop, b""),
        "/xhr" => {
            let page = PAGE.replace("OTHER", &format!("'{other}'"));
            respond(s, "200 OK", "Content-Type: text/html\r\nSet-Cookie: sid=1; Path=/\r\n", page.as_bytes());
        }
        p if p.starts_with("/echo") => {
            let text = format!("{} {} x-test={} cookie={} body={}", r.method, p, r.header("x-test"), r.header("cookie"), String::from_utf8_lossy(&r.body));
            respond(s, "200 OK", "Content-Type: text/plain\r\nX-Echo: e\r\nSet-Cookie: got=1; Path=/\r\n", text.as_bytes());
            served.fetch_add(1, Ordering::SeqCst);
        }
        "/wait.js" => {
            // the page's load waits for this script, which waits for the
            // four requests to be answered and their responses to arrive
            let t0 = Instant::now();
            while served.load(Ordering::SeqCst) < 4 && t0.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(10));
            }
            std::thread::sleep(Duration::from_millis(300));
            respond(s, "200 OK", "Content-Type: text/javascript\r\n", b"log('waited')");
        }
        _ => respond(s, "404 Not Found", "", b""),
    })
}

fn check(h: &mut Host, origin: &str, how: &str) {
    let (opened, error) = open_at(h, &format!("{origin}/xhr"));
    assert!(!error, "{how}: {opened}");
    let page = opened.split_whitespace().nth(1).unwrap().to_owned();
    let text = h.call("read", &format!(r#"{{"page":"{page}"}}"#));
    // the hop to another origin carries no cookie and the page's origin
    let hop = format!("|hop cookie= origin={origin}");
    let open = format!("|open cookie= origin={origin} -1");
    for want in ["|200 OK e POST /echo?a=1 x-test=t1 cookie=sid=1 body=body-1 jar=sid=1; got=1", &open, "|closed error 0", &hop, "|waited"] {
        assert!(text.contains(want), "{how}: {want} in {text}");
    }
    let log = h.call("requests", &format!(r#"{{"page":"{page}"}}"#));
    for want in [format!("xhr POST {origin}/echo?a=1 200"), "xhr GET http://127.0.0.1:".to_owned()] {
        assert!(log.contains(&want), "{how}: {want} in {log}");
    }
}

/// An XMLHttpRequest's request goes through the network service with
/// its method, own headers, body and the session's cookies, and the
/// cookies its response sets are stored; one to another origin goes
/// without cookies and is read only when the response allows it.
#[test]
fn xhr_requests_go_through_the_network_service() {
    let origin = sites();
    let mut h = Host::start();
    check(&mut h, &origin, "in the host's process");
    let origin = sites();
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mcc-xhr-{}", std::process::id()));
    let (_child, mut h) = executable(&dir, &[]);
    check(&mut h, &origin, "in a content process");
}

/// A request still in flight when its document has loaded is read to its
/// end: a wait waits for it, and the page's request log shows it.
#[test]
fn a_request_in_flight_past_the_load_is_waited_for() {
    let origin = serve(|r, s| match r.path.as_str() {
        "/late" => {
            let page = b"<!doctype html><title>Late</title><script>var x = new XMLHttpRequest(); x.open('GET', '/slow'); x.send();</script>";
            respond(s, "200 OK", "Content-Type: text/html\r\n", page);
        }
        "/slow" => {
            std::thread::sleep(Duration::from_millis(500));
            respond(s, "200 OK", "", b"slow body");
        }
        _ => respond(s, "404 Not Found", "", b""),
    });
    let mut h = Host::start();
    let (opened, error) = open_at(&mut h, &format!("{origin}/late"));
    assert!(!error, "{opened}");
    let page = opened.split_whitespace().nth(1).unwrap().to_owned();
    let t0 = Instant::now();
    let waited = h.call("wait", &format!(r#"{{"page":"{page}","timeout_ms":5000}}"#));
    assert!(!waited.contains("in flight") && t0.elapsed() < Duration::from_secs(4), "{waited} after {:?}", t0.elapsed());
    let log = h.call("requests", &format!(r#"{{"page":"{page}"}}"#));
    assert!(log.contains(&format!("xhr GET {origin}/slow 200")), "{log}");
}
