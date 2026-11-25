//! The network process over its Unix socket, against a local HTTP server.

use madcawl_net::wire::{Frame, kind, tag};
use madcawl_net::{Client, Config};
use madcawl_netd::{ServerLimits, serve, serve_streams};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// A one-request-per-connection HTTP server: `/login` sets a plain and an
/// HttpOnly cookie, `/big` sends `n` bytes, anything else echoes `Cookie`.
fn http_fixture() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut buf = Vec::new();
            let mut b = [0u8; 4096];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = s.read(&mut b).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&b[..n]);
            }
            let text = String::from_utf8_lossy(&buf).into_owned();
            let path = text.split(' ').nth(1).unwrap_or("/").to_owned();
            let cookie = text
                .lines()
                .find_map(|l| l.strip_prefix("Cookie: "))
                .unwrap_or("")
                .to_owned();
            let (extra, body) = match path.as_str() {
                "/login" => ("Set-Cookie: sid=s3cret; HttpOnly\r\nSet-Cookie: theme=dark\r\n", Vec::new()),
                "/big" => ("", vec![b'z'; 600 * 1024]),
                _ => ("", cookie.into_bytes()),
            };
            let head = format!("HTTP/1.1 200 OK\r\nConnection: close\r\n{extra}Content-Length: {}\r\n\r\n", body.len());
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&body);
        }
    });
    format!("http://{addr}")
}

static N: AtomicUsize = AtomicUsize::new(0);

fn netd(limits: ServerLimits) -> PathBuf {
    let path = std::env::temp_dir().join(format!("mcnetd-{}-{}.sock", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_file(&path);
    let l = UnixListener::bind(&path).unwrap();
    let client = Arc::new(Client::new(Config::default()));
    std::thread::spawn(move || serve(l, client, limits));
    path
}

fn connect(path: &PathBuf) -> UnixStream {
    let s = UnixStream::connect(path).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s
}

fn read(s: &mut UnixStream) -> Frame {
    Frame::read_from(s).unwrap().expect("a frame")
}

/// Fetches `url`; the head frame and the joined body.
fn fetch(s: &mut UnixStream, id: u64, url: &str) -> (Frame, Vec<u8>) {
    Frame::new(kind::FETCH).num(tag::ID, id).str(tag::URL, url).write_to(s).unwrap();
    let head = read(s);
    assert_eq!(head.kind, kind::HEAD, "{head:?}");
    let mut body = Vec::new();
    let mut chunks = 0;
    loop {
        let f = read(s);
        assert_eq!(f.get_num(tag::ID), Some(id));
        match f.kind {
            kind::BODY => {
                body.extend_from_slice(f.field(tag::BODY).unwrap());
                chunks += 1;
            }
            kind::END => break,
            k => panic!("unexpected frame kind {k}"),
        }
    }
    assert!(chunks <= body.len().div_ceil(madcawl_net::wire::BODY_CHUNK));
    (head, body)
}

fn cookie_get(s: &mut UnixStream, url: &str) -> String {
    Frame::new(kind::COOKIE_GET).num(tag::ID, 9).str(tag::URL, url).write_to(s).unwrap();
    read(s).get_str(tag::TEXT).unwrap().to_owned()
}

#[test]
fn http_only_cookies_never_leave_the_network_process() {
    let origin = http_fixture();
    let path = netd(ServerLimits::default());
    let mut s = connect(&path);
    let (head, _) = fetch(&mut s, 1, &format!("{origin}/login"));
    assert_eq!(head.get_num(tag::STATUS), Some(200));
    assert!(head.headers().iter().all(|(n, _)| !n.eq_ignore_ascii_case("set-cookie")));
    assert_eq!(cookie_get(&mut s, &format!("{origin}/")), "theme=dark");
    let (_, echoed) = fetch(&mut s, 2, &format!("{origin}/echo"));
    assert_eq!(echoed, b"sid=s3cret; theme=dark", "the jar sends both on the wire");
    let set = |s: &mut UnixStream, line: &str| {
        Frame::new(kind::COOKIE_SET).num(tag::ID, 3).str(tag::URL, &format!("{origin}/")).str(tag::TEXT, line).write_to(s).unwrap();
        read(s).get_num(tag::OK)
    };
    assert_eq!(set(&mut s, "sid=overwritten"), Some(0), "script cannot overwrite an HttpOnly cookie");
    assert_eq!(set(&mut s, "x=1; HttpOnly"), Some(0), "script cannot set an HttpOnly cookie");
    assert_eq!(set(&mut s, "lang=en"), Some(1));
    assert_eq!(cookie_get(&mut s, &format!("{origin}/")), "theme=dark; lang=en");
    let (_, echoed) = fetch(&mut s, 4, &format!("{origin}/echo"));
    assert_eq!(echoed, b"sid=s3cret; theme=dark; lang=en");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn large_bodies_errors_and_unknown_frames() {
    let origin = http_fixture();
    let path = netd(ServerLimits::default());
    let mut s = connect(&path);
    Frame::new(0x7777).num(tag::ID, 5).write_to(&mut s).unwrap();
    let (_, body) = fetch(&mut s, 6, &format!("{origin}/big"));
    assert_eq!(body.len(), 600 * 1024);
    Frame::new(kind::FETCH).num(tag::ID, 7).str(tag::URL, "file:///etc/passwd").write_to(&mut s).unwrap();
    let e = read(&mut s);
    assert_eq!((e.kind, e.get_num(tag::ID)), (kind::ERROR, Some(7)));
    Frame::new(kind::FETCH).num(tag::ID, 8).write_to(&mut s).unwrap();
    assert_eq!(read(&mut s).kind, kind::ERROR);
    // a malformed frame ends this client only
    s.write_all(&[0xff, 0xff, 0xff, 0x7f, 0, 0]).unwrap();
    let mut b = [0u8; 1];
    assert_eq!(s.read(&mut b).unwrap(), 0);
    let mut t = connect(&path);
    assert_eq!(cookie_get(&mut t, &format!("{origin}/")), "");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn clients_and_requests_in_flight_are_capped() {
    let path = netd(ServerLimits { clients: 1, in_flight: 1, ..ServerLimits::default() });
    let mut a = connect(&path);
    assert_eq!(cookie_get(&mut a, "http://127.0.0.1/"), "");
    // no read timeout: the server may have closed it already, and macOS
    // refuses socket options on a socket whose peer is gone (EINVAL)
    let mut b = UnixStream::connect(&path).unwrap();
    let mut one = [0u8; 1];
    assert_eq!(b.read(&mut one).unwrap(), 0, "a second client is turned away");
    // a request that hangs on a server that never answers holds the one slot
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", silent.local_addr().unwrap());
    Frame::new(kind::FETCH).num(tag::ID, 1).str(tag::URL, &url).write_to(&mut a).unwrap();
    Frame::new(kind::FETCH).num(tag::ID, 2).str(tag::URL, &url).write_to(&mut a).unwrap();
    let e = read(&mut a);
    assert_eq!((e.kind, e.get_num(tag::ID)), (kind::ERROR, Some(2)));
    drop(silent);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_forbidden_method_is_answered_with_an_error() {
    let origin = http_fixture();
    let path = netd(ServerLimits::default());
    let mut s = connect(&path);
    Frame::new(kind::FETCH).num(tag::ID, 1).str(tag::METHOD, "trace").str(tag::URL, &format!("{origin}/")).write_to(&mut s).unwrap();
    let e = read(&mut s);
    assert_eq!((e.kind, e.get_num(tag::ID)), (kind::ERROR, Some(1)));
    assert!(e.get_str(tag::MESSAGE).unwrap().contains("forbidden method"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_failed_accept_does_not_stop_the_server() {
    let origin = http_fixture();
    let path = std::env::temp_dir().join(format!("mcnetd-{}-accept.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let l = UnixListener::bind(&path).unwrap();
    let client = Arc::new(Client::new(Config::default()));
    let failures = [24, 23, 53].map(|e| Err(std::io::Error::from_raw_os_error(e)));
    std::thread::spawn(move || serve_streams(failures.into_iter().chain(l.incoming()), client, ServerLimits::default()));
    let mut s = connect(&path);
    assert_eq!(cookie_get(&mut s, &format!("{origin}/")), "");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn requests_are_capped_across_clients() {
    let path = netd(ServerLimits { requests: 1, ..ServerLimits::default() });
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", silent.local_addr().unwrap());
    let mut a = connect(&path);
    let mut b = connect(&path);
    Frame::new(kind::FETCH).num(tag::ID, 1).str(tag::URL, &url).write_to(&mut a).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    Frame::new(kind::FETCH).num(tag::ID, 2).str(tag::URL, &url).write_to(&mut b).unwrap();
    let e = read(&mut b);
    assert_eq!((e.kind, e.get_num(tag::ID)), (kind::ERROR, Some(2)), "the one place is taken by another client");
    drop(silent);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_client_that_stops_reading_lets_go_of_its_request() {
    let origin = http_fixture();
    let limits = ServerLimits { requests: 1, write_timeout: Duration::from_millis(200), ..ServerLimits::default() };
    let path = netd(limits);
    let mut stuck = connect(&path);
    Frame::new(kind::FETCH).num(tag::ID, 1).str(tag::URL, &format!("{origin}/big")).write_to(&mut stuck).unwrap();
    std::thread::sleep(Duration::from_millis(1000));
    // the stuck client's request gave its place back once its writes timed out
    let mut s = connect(&path);
    let (head, body) = fetch(&mut s, 2, &format!("{origin}/echo"));
    assert_eq!((head.get_num(tag::STATUS), body), (Some(200), Vec::new()));
    // and the stuck client finds its connection closed part way
    let mut rest = Vec::new();
    stuck.read_to_end(&mut rest).unwrap();
    assert!(rest.len() < 600 * 1024);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_client_that_disconnects_gives_its_requests_back_at_once() {
    let origin = http_fixture();
    let path = netd(ServerLimits { requests: 2, ..ServerLimits::default() });
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", silent.local_addr().unwrap());
    let mut a = connect(&path);
    for id in 1..=2 {
        Frame::new(kind::FETCH).num(tag::ID, id).str(tag::URL, &url).write_to(&mut a).unwrap();
    }
    std::thread::sleep(Duration::from_millis(200));
    let mut b = connect(&path);
    Frame::new(kind::FETCH).num(tag::ID, 3).str(tag::URL, &format!("{origin}/echo")).write_to(&mut b).unwrap();
    assert_eq!(read(&mut b).kind, kind::ERROR, "both places are held by the hanging requests");
    drop(a);
    std::thread::sleep(Duration::from_millis(300));
    let (head, _) = fetch(&mut b, 4, &format!("{origin}/echo"));
    assert_eq!(head.get_num(tag::STATUS), Some(200), "the gone client's requests were cancelled");
    drop(silent);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn one_client_cannot_take_another_clients_reserve() {
    let origin = http_fixture();
    // each of the two clients keeps one request for itself; two more are shared
    let limits = ServerLimits { clients: 2, in_flight: 4, requests: 4, ..ServerLimits::default() };
    assert_eq!(limits.reserve(), 1);
    let path = netd(limits);
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", silent.local_addr().unwrap());
    let mut a = connect(&path);
    for id in 1..=4 {
        Frame::new(kind::FETCH).num(tag::ID, id).str(tag::URL, &url).write_to(&mut a).unwrap();
    }
    let e = read(&mut a);
    assert_eq!((e.kind, e.get_num(tag::ID)), (kind::ERROR, Some(4)));
    let mut b = connect(&path);
    let (head, _) = fetch(&mut b, 5, &format!("{origin}/echo"));
    assert_eq!(head.get_num(tag::STATUS), Some(200));
    drop(silent);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn request_bodies_are_held_within_a_budget() {
    let path = netd(ServerLimits { body_bytes: 1 << 20, ..ServerLimits::default() });
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", silent.local_addr().unwrap());
    let mut a = connect(&path);
    let post = |s: &mut UnixStream, id: u64| {
        let f = Frame::new(kind::FETCH).num(tag::ID, id).str(tag::METHOD, "POST").str(tag::URL, &url);
        f.bytes(tag::BODY, &vec![b'b'; 700 << 10]).write_to(s).unwrap();
    };
    post(&mut a, 1);
    std::thread::sleep(Duration::from_millis(100));
    let mut b = connect(&path);
    post(&mut b, 2);
    let e = read(&mut b);
    assert_eq!((e.kind, e.get_num(tag::ID)), (kind::ERROR, Some(2)));
    assert!(e.get_str(tag::MESSAGE).unwrap().contains("budget"));
    drop(silent);
    let _ = std::fs::remove_file(&path);
}
