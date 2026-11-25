//! A small server-rendered site on 127.0.0.1 for the profile, download
//! and form tests, and a client for the network process's socket and its
//! control channel.
//!
//! `/login` is a login form with a CSRF token; posting the right fields
//! to `/session` sets a persistent `sid` cookie and a session `visit`
//! cookie and redirects to `/home`, which greets only a request carrying
//! `sid`. `/echo` answers a request's method, query, `Content-Type` and
//! body. `/attach`, `/doc.pdf`, `/huge` and `/stream` are downloads;
//! `/away` redirects to `/attach` on `localhost`, another origin, and
//! `/slow` is a 300 000-byte attachment of unannounced length whose body
//! starts 300 ms after its head.

#![allow(dead_code)]

use madcawl_net::wire::{Frame, kind, tag};
use madcawl_net::{Client, Config};
use madcawl_netd::{Downloads, ServerLimits, serve_guarded};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub const LOGIN_BODY: &str = "user=alice&pass=s3cret&csrf=tok123&go=1";
pub const PDF_LEN: usize = 2_621_440 + 7;

const LOGIN_PAGE: &str = r#"<!doctype html><title>Log in</title>
<form method=post action="/session"><input name=user><input type=password name=pass>
<input type=hidden name=csrf value=tok123><input type=checkbox name=remember>
<button type=submit name=go value=1 id=go>Log in</button></form>"#;

struct Req {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Req {
    fn header(&self, name: &str) -> &str {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map_or("", |(_, v)| v.as_str())
    }
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
        lines.filter_map(|l| l.split_once(':')).map(|(n, v)| (n.trim().to_owned(), v.trim().to_owned())).collect();
    let mut body = buf[end + 4..].to_vec();
    let len = headers.iter().find(|(n, _)| n.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    while body.len() < len {
        let n = s.read(&mut b).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&b[..n]);
    }
    Some(Req { method, target, headers, body })
}

fn respond(s: &mut TcpStream, status: &str, extra: &str, body: &[u8]) {
    let head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n{extra}Content-Length: {}\r\n\r\n", body.len());
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

fn handle(mut s: TcpStream) {
    let Some(r) = read_req(&mut s) else { return };
    let (path, query) = r.target.split_once('?').unwrap_or((&r.target, ""));
    match (r.method.as_str(), path) {
        ("GET", "/login") => respond(&mut s, "200 OK", "Content-Type: text/html\r\n", LOGIN_PAGE.as_bytes()),
        ("POST", "/session") if r.body == LOGIN_BODY.as_bytes() && r.header("content-type") == "application/x-www-form-urlencoded" => {
            let cookies = "Set-Cookie: sid=abc123; Max-Age=3600; HttpOnly; Path=/\r\nSet-Cookie: visit=1; Path=/\r\n";
            respond(&mut s, "303 See Other", &format!("{cookies}Location: /home\r\n"), b"");
        }
        ("POST", "/session") => respond(&mut s, "400 Bad Request", "", &r.body),
        ("GET", "/home") => {
            let cookie = r.header("cookie");
            match cookie.split("; ").any(|c| c == "sid=abc123") {
                true => respond(&mut s, "200 OK", "", format!("Welcome alice; cookies: {cookie}").as_bytes()),
                false => respond(&mut s, "403 Forbidden", "", b"log in first"),
            }
        }
        (m, "/echo") => {
            let text = format!("{m} {query}\n{}\n{}", r.header("content-type"), String::from_utf8_lossy(&r.body));
            respond(&mut s, "200 OK", "Content-Type: text/plain\r\n", text.as_bytes());
        }
        ("GET", "/attach") => {
            let cd = "Content-Disposition: attachment; filename=\"../../evil/.report.pdf\"\r\nContent-Type: text/html\r\n";
            respond(&mut s, "200 OK", cd, b"PDFDATA");
        }
        ("GET", "/away") => {
            let port = r.header("host").rsplit(':').next().unwrap_or("");
            respond(&mut s, "302 Found", &format!("Location: http://localhost:{port}/attach\r\n"), b"");
        }
        ("GET", "/doc.pdf") => respond(&mut s, "200 OK", "Content-Type: application/pdf\r\n", &vec![b'p'; PDF_LEN]),
        ("GET", "/huge") => respond(&mut s, "200 OK", "Content-Disposition: attachment\r\n", &vec![b'h'; 300_000]),
        ("GET", "/slow") => {
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Disposition: attachment\r\n\r\n");
            std::thread::sleep(Duration::from_millis(300));
            let _ = s.write_all(&vec![b's'; 300_000]);
        }
        ("GET", "/stream") => {
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Disposition: attachment\r\n\r\n");
            let _ = s.write_all(&vec![b's'; 300_000]);
        }
        _ => respond(&mut s, "404 Not Found", "", b"no such page"),
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

static N: AtomicUsize = AtomicUsize::new(0);

/// A fresh path under the temporary directory, short enough for a
/// socket address.
pub fn temp_path(what: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mcp{}-{}-{what}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)))
}

pub fn connect(path: &Path) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match UnixStream::connect(path) {
            Ok(s) => {
                s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                return s;
            }
            Err(e) if Instant::now() > deadline => panic!("connect {}: {e}", path.display()),
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Every frame of one request's answer, up to END or ERROR.
pub fn exchange(s: &mut UnixStream, f: Frame) -> Vec<Frame> {
    f.write_to(s).unwrap();
    let mut out = Vec::new();
    loop {
        let f = Frame::read_from(s).unwrap().expect("a frame");
        let k = f.kind;
        out.push(f);
        if k == kind::END || k == kind::ERROR {
            return out;
        }
    }
}

pub fn get(s: &mut UnixStream, id: u64, url: &str) -> Vec<Frame> {
    exchange(s, Frame::new(kind::FETCH).num(tag::ID, id).str(tag::URL, url))
}

/// The status and joined body of an answer.
pub fn status_body(frames: &[Frame]) -> (u64, String) {
    let status = frames.iter().find(|f| f.kind == kind::HEAD).and_then(|f| f.get_num(tag::STATUS)).unwrap_or(0);
    let body: Vec<u8> = frames.iter().filter(|f| f.kind == kind::BODY).flat_map(|f| f.field(tag::BODY).unwrap().to_vec()).collect();
    (status, String::from_utf8_lossy(&body).into_owned())
}

pub fn navigate(s: &mut UnixStream, id: u64, url: &str) -> Vec<Frame> {
    exchange(s, Frame::new(kind::FETCH).num(tag::ID, id).str(tag::URL, url).num(tag::NAVIGATION, 1))
}

/// Registers a document at `url` on the control channel, or the host's
/// own connection when `url` is `None`; the token.
pub fn register(control: &mut UnixStream, url: Option<&str>) -> Vec<u8> {
    let f = Frame::new(kind::REGISTER).num(tag::ID, 1);
    let f = match url {
        Some(u) => f.str(tag::URL, u),
        None => f.num(tag::NAVIGATION, 1),
    };
    f.write_to(control).unwrap();
    let r = Frame::read_from(control).unwrap().expect("a frame");
    assert_eq!(r.kind, kind::REGISTERED, "{r:?}");
    r.field(tag::TOKEN).unwrap().to_vec()
}

/// A connection to the socket at `path`, bound with HELLO to `token`.
pub fn hello(path: &Path, token: &[u8]) -> UnixStream {
    let mut s = connect(path);
    Frame::new(kind::HELLO).num(tag::ID, 0).bytes(tag::TOKEN, token).write_to(&mut s).unwrap();
    let r = Frame::read_from(&mut s).unwrap().expect("a frame");
    assert_eq!((r.kind, r.get_num(tag::OK)), (kind::HELLO, Some(1)), "{r:?}");
    s
}

/// The ERROR a guarded network process refuses a frame with.
pub fn assert_refused(frames: &[Frame]) {
    let last = frames.last().unwrap();
    assert_eq!((last.kind, last.get_num(tag::REFUSED)), (kind::ERROR, Some(1)), "{frames:?}");
}

/// A network process in this one, served with a control channel.
pub struct Guarded {
    pub path: PathBuf,
    pub control: UnixStream,
}

impl Drop for Guarded {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn guarded(downloads: Option<Downloads>) -> Guarded {
    let path = temp_path("s");
    let l = UnixListener::bind(&path).unwrap();
    let (host, theirs) = UnixStream::pair().unwrap();
    host.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let (client, d) = (Arc::new(Client::new(Config::default())), downloads.map(Arc::new));
    std::thread::spawn(move || serve_guarded(l, theirs, client, ServerLimits::default(), d).unwrap());
    Guarded { path, control: host }
}

impl Guarded {
    /// The host's own connection, which navigations are fetched on.
    pub fn host(&mut self) -> UnixStream {
        let t = register(&mut self.control, None);
        hello(&self.path, &t)
    }

    /// A content process's connection for the document at `url`.
    pub fn content(&mut self, url: &str) -> UnixStream {
        let t = register(&mut self.control, Some(url));
        hello(&self.path, &t)
    }
}
