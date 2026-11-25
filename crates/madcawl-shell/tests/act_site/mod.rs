//! A small server-rendered site on 127.0.0.1 for the action tests, and a
//! host started with the network on.
//!
//! `/` links to the others. `/login` posts `user` and `pass` to
//! `/session`, which sets `sid` and redirects to `/home` for alice with
//! s3cret; `/home` greets only a request with `sid`. `/form` has one of
//! each control, a disabled button, a hidden link, a button under an
//! overlay and a file input posting multipart to `/echo`, which answers
//! a request's method, query and body as plain text. `/long` has a link
//! far below the first screen. `/slow` answers after 1.5 s; `/notes.txt`
//! is an attachment. `/gate` is a GET form with a password field going
//! to `/gated`, a page with a stylesheet; `/hop` a password form posting
//! to `/keep`, which answers 307 to `/echo` on `localhost`, another
//! origin (the POSTs `/echo` gets there are counted in `LOCALHOST_POSTS`).
//! `/files?p=<path>` has a link and a form to `file://<path>`. `/moved`
//! has a password field the parser leaves outside its form but owned by
//! it, the form going to `http://evil/`; `/notform` a password field
//! whose `form` attribute names an ID whose first element is not a
//! form, beside a GET form to `/echo` with that ID. Anything else is a
//! 404.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[path = "../serve_fixture/mod.rs"]
mod serve_fixture;
pub use serve_fixture::*;

pub const HOME: &str = r#"<!doctype html><title>Site</title><h1>Site</h1><nav>
<a href="/login">Log in</a> <a href="/form">Form</a> <a href="/long">Long</a> <a href="/slow">Slow</a>
<a href="/notes.txt">Notes</a> <a href="/missing">Missing</a></nav>"#;

const LOGIN: &str = r#"<!doctype html><title>Log in</title><form method=post action=/session>
<label>User <input name=user></label><label>Password <input type=password name=pass></label>
<button>Log in</button></form>"#;

const FORM: &str = r#"<!doctype html><title>Form</title>
<form method=post action=/echo enctype=multipart/form-data>
<label>Title <input name=title value=draft></label>
<label>Body <textarea name=body></textarea></label>
<label><input type=checkbox name=notify> Notify me</label>
<label><input type=radio name=size value=s checked> Small</label>
<label><input type=radio name=size value=l> Large</label>
<label>Lang <select name=lang><option>Rust<option value=py>Python</select></label>
<label>File <input type=file name=doc></label>
<button disabled>Frozen</button>
<a href="/x" style="display:none">Hidden link</a>
<div style="position:relative;height:40px"><button type=button>Under</button>
<div style="position:absolute;left:0;top:0;width:400px;height:40px;background:white">Cover</div></div>
<button>Send</button></form>"#;

const GATE: &str = r#"<!doctype html><title>Gate</title><form action=/gated>
<label>User <input name=user></label><label>PIN <input type=password name=pin></label><button>Enter</button></form>"#;

const HOP: &str = r#"<!doctype html><title>Hop</title><form method=post action=/keep>
<label>Password <input type=password name=pass></label><button>Go</button></form>"#;

const MOVED: &str = r#"<!doctype html><title>Moved</title><div><form action=//evil/ method=post><button>Go</button></div>
<table><tr><td><label>Secret <input type=password name=p></label></table>"#;

const NOT_FORM: &str = r#"<!doctype html><title>Not a form</title><p id=x></p><form id=x action=/echo>
<label>User <input name=user></label><button>Send</button></form><label>Secret <input type=password form=x name=p></label>"#;

/// POSTs to `/echo` that came by the name `localhost`
pub static LOCALHOST_POSTS: AtomicUsize = AtomicUsize::new(0);

const LONG: &str = r#"<!doctype html><title>Long</title><h1>Top</h1>
<div style="height:3000px">filler</div><a href="/">Far link</a>"#;

struct Req {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_req(s: &mut TcpStream) -> Option<Req> {
    let mut buf = Vec::new();
    let mut b = [0u8; 8192];
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = s.read(&mut b).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&b[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?.to_owned(), first.next()?.to_owned());
    let headers: Vec<(String, String)> =
        lines.filter_map(|l| l.split_once(':')).map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned())).collect();
    let len = headers.iter().find(|(n, _)| n == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    let mut body = buf[end + 4..].to_vec();
    while body.len() < len {
        let n = s.read(&mut b).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&b[..n]);
    }
    Some(Req { method, target, headers, body })
}

fn header<'a>(r: &'a Req, name: &str) -> &'a str {
    r.headers.iter().find(|(n, _)| n == name).map_or("", |(_, v)| v.as_str())
}

fn respond(s: &mut TcpStream, status: &str, extra: &str, body: &[u8]) {
    let head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n{extra}Content-Length: {}\r\n\r\n", body.len());
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

const HTML: &str = "Content-Type: text/html; charset=utf-8\r\n";

fn handle(mut s: TcpStream) {
    let Some(r) = read_req(&mut s) else { return };
    let (path, query) = r.target.split_once('?').unwrap_or((&r.target, ""));
    let cookie = r.headers.iter().find(|(n, _)| n == "cookie").map_or("", |(_, v)| v.as_str());
    match (r.method.as_str(), path) {
        ("GET", "/") => respond(&mut s, "200 OK", HTML, HOME.as_bytes()),
        ("GET", "/login") => respond(&mut s, "200 OK", HTML, LOGIN.as_bytes()),
        ("POST", "/session") if r.body == b"user=alice&pass=s3cret" => {
            respond(&mut s, "303 See Other", "Set-Cookie: sid=abc; Path=/; HttpOnly\r\nLocation: /home\r\n", b"");
        }
        ("POST", "/session") => respond(&mut s, "401 Unauthorized", HTML, b"<p>Wrong user or password</p>"),
        ("GET", "/home") if cookie.contains("sid=abc") => respond(&mut s, "200 OK", HTML, b"<h1>Welcome alice</h1>"),
        ("GET", "/home") => respond(&mut s, "403 Forbidden", HTML, b"<p>Log in first</p>"),
        ("GET", "/form") => respond(&mut s, "200 OK", HTML, FORM.as_bytes()),
        ("GET", "/gate") => respond(&mut s, "200 OK", HTML, GATE.as_bytes()),
        ("GET", "/gated") => {
            respond(&mut s, "200 OK", HTML, b"<!doctype html><link rel=stylesheet href=gate.css><h1>Gate open</h1>");
        }
        ("GET", "/gate.css") => respond(&mut s, "200 OK", "Content-Type: text/css\r\n", b"h1{color:green}"),
        ("GET", "/hop") => respond(&mut s, "200 OK", HTML, HOP.as_bytes()),
        ("GET", "/moved") => respond(&mut s, "200 OK", HTML, MOVED.as_bytes()),
        ("GET", "/notform") => respond(&mut s, "200 OK", HTML, NOT_FORM.as_bytes()),
        (_, "/keep") => {
            let port = header(&r, "host").rsplit(':').next().unwrap_or("").to_owned();
            respond(&mut s, "307 Temporary Redirect", &format!("Location: http://localhost:{port}/echo\r\n"), b"");
        }
        ("GET", "/files") => {
            let p = query.strip_prefix("p=").unwrap_or("");
            let page = format!("<!doctype html><a href=\"file://{p}\">Local</a><form action=\"file://{p}\"><button>Go local</button></form>");
            respond(&mut s, "200 OK", HTML, page.as_bytes());
        }
        (m, "/echo") => {
            if m == "POST" && header(&r, "host").starts_with("localhost") {
                LOCALHOST_POSTS.fetch_add(1, Ordering::SeqCst);
            }
            let text = format!("{m} {query}\n{}", String::from_utf8_lossy(&r.body));
            respond(&mut s, "200 OK", "Content-Type: text/plain\r\n", text.as_bytes());
        }
        ("GET", "/long") => respond(&mut s, "200 OK", HTML, LONG.as_bytes()),
        ("GET", "/slow") => {
            std::thread::sleep(Duration::from_millis(1500));
            respond(&mut s, "200 OK", HTML, b"<h1>Slow page</h1>");
        }
        ("GET", "/notes.txt") => {
            respond(&mut s, "200 OK", "Content-Disposition: attachment; filename=notes.txt\r\n", b"line one\nline two\n");
        }
        _ => respond(&mut s, "404 Not Found", HTML, b"<p>No such page</p>"),
    }
}

/// Starts the site; its origin, `http://127.0.0.1:<port>`.
pub fn site() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || handle(s));
        }
    });
    format!("http://{addr}")
}

/// A host with the network on, uploads allowed from its directory's
/// `up`, password forms approved for `origin`; started.
pub fn host(origin: &str) -> Fixture {
    let mut f = Fixture::new();
    let up = f.dir.join("up");
    std::fs::create_dir_all(&up).unwrap();
    f.config.net = true;
    f.config.downloads = Some(f.dir.join("downloads"));
    f.config.uploads = vec![up.canonicalize().unwrap()];
    f.config.approve = vec![origin.to_owned()];
    f.start();
    f
}

/// The page id `opened pN` gives.
pub fn page_of(opened: &str) -> String {
    opened.split_whitespace().nth(1).unwrap().to_owned()
}

/// The ref on the first line with a quoted name starting `text`, e.g.
/// `e5`.
pub fn ref_on(reply: &str, text: &str) -> String {
    let quoted = format!("\"{text}");
    let line = reply.lines().find(|l| l.contains(&quoted) && l.contains("[e"));
    let line = line.unwrap_or_else(|| panic!("no {text:?} with a ref in {reply}"));
    let at = line.find("[e").expect("a ref");
    line[at + 1..].split(']').next().unwrap().to_owned()
}

pub fn act(m: &mut Mcp, page: &str, args: &str) -> (String, bool) {
    m.call("act", &format!(r#"{{"page":"{page}",{args}}}"#))
}
