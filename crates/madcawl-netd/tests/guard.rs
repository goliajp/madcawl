//! The network process checking what content processes claim against the
//! documents their host registered on the control channel.

use madcawl_net::wire::{Frame, kind, tag};
use madcawl_net::{Client, Config};
use madcawl_netd::{ServerLimits, serve_guarded};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// `/login` sets a cookie (SameSite absent, so Lax); anything else echoes
/// the request's `Cookie`.
fn http_fixture() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
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
            let cookie = text.lines().find_map(|l| l.strip_prefix("Cookie: ")).unwrap_or("").to_owned();
            let (extra, body) = match text.split(' ').nth(1) {
                Some("/login") => ("Set-Cookie: theme=dark\r\n", String::new()),
                _ => ("", cookie),
            };
            let head = format!("HTTP/1.1 200 OK\r\nConnection: close\r\n{extra}Content-Length: {}\r\n\r\n", body.len());
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(body.as_bytes());
        }
    });
    port
}

static N: AtomicUsize = AtomicUsize::new(0);

struct Netd {
    path: PathBuf,
    control: UnixStream,
}

impl Drop for Netd {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn netd(limits: ServerLimits) -> Netd {
    let path = std::env::temp_dir().join(format!("mcguard-{}-{}.sock", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_file(&path);
    let l = UnixListener::bind(&path).unwrap();
    let (host, theirs) = UnixStream::pair().unwrap();
    host.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let client = Arc::new(Client::new(Config::default()));
    std::thread::spawn(move || serve_guarded(l, theirs, client, limits, None).unwrap());
    Netd { path, control: host }
}

fn read(s: &mut UnixStream) -> Frame {
    Frame::read_from(s).unwrap().expect("a frame")
}

impl Netd {
    /// Registers `url`, or moves the registration `token` to it.
    fn register(&mut self, url: &str, token: Option<&[u8]>) -> Frame {
        let mut f = Frame::new(kind::REGISTER).num(tag::ID, 1).str(tag::URL, url);
        if let Some(t) = token {
            f = f.bytes(tag::TOKEN, t);
        }
        f.write_to(&mut self.control).unwrap();
        read(&mut self.control)
    }

    fn token(&mut self, url: &str) -> Vec<u8> {
        let r = self.register(url, None);
        assert_eq!(r.kind, kind::REGISTERED, "{r:?}");
        r.field(tag::TOKEN).unwrap().to_vec()
    }

    fn refused(&mut self, token: Option<&[u8]>) -> u64 {
        let mut f = Frame::new(kind::STATS).num(tag::ID, 2);
        if let Some(t) = token {
            f = f.bytes(tag::TOKEN, t);
        }
        f.write_to(&mut self.control).unwrap();
        read(&mut self.control).get_num(tag::COUNT).unwrap()
    }

    /// A content process's connection, introduced with `token`.
    fn content(&self, token: &[u8]) -> UnixStream {
        let mut s = UnixStream::connect(&self.path).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        Frame::new(kind::HELLO).num(tag::ID, 0).bytes(tag::TOKEN, token).write_to(&mut s).unwrap();
        let hello = read(&mut s);
        assert_eq!((hello.kind, hello.get_num(tag::OK)), (kind::HELLO, Some(1)), "{hello:?}");
        s
    }
}

fn assert_refused(f: &Frame, id: u64) {
    assert_eq!((f.kind, f.get_num(tag::ID), f.get_num(tag::REFUSED)), (kind::ERROR, Some(id), Some(1)), "{f:?}");
}

/// Fetches `url` claiming `initiator`; the joined body, or the error frame.
fn fetch(s: &mut UnixStream, id: u64, url: &str, initiator: Option<&str>) -> Result<String, Frame> {
    let mut f = Frame::new(kind::FETCH).num(tag::ID, id).str(tag::URL, url).num(tag::NAVIGATION, 0);
    if let Some(i) = initiator {
        f = f.str(tag::INITIATOR, i);
    }
    exchange(s, f)
}

/// Sends a FETCH frame as given; the joined body, or the error frame.
fn exchange(s: &mut UnixStream, f: Frame) -> Result<String, Frame> {
    f.write_to(s).unwrap();
    let head = read(s);
    if head.kind != kind::HEAD {
        return Err(head);
    }
    let mut body = Vec::new();
    loop {
        let f = read(s);
        match f.kind {
            kind::BODY => body.extend_from_slice(f.field(tag::BODY).unwrap()),
            kind::END => return Ok(String::from_utf8(body).unwrap()),
            _ => return Err(f),
        }
    }
}

fn cookie_get(s: &mut UnixStream, url: &str) -> Frame {
    Frame::new(kind::COOKIE_GET).num(tag::ID, 9).str(tag::URL, url).write_to(s).unwrap();
    read(s)
}

#[test]
fn a_forged_initiator_is_refused_and_counted() {
    let port = http_fixture();
    let mut netd = netd(ServerLimits::default());
    let doc = format!("http://127.0.0.1:{port}/page");
    let token = netd.token(&doc);
    let mut s = netd.content(&token);
    let login = format!("http://127.0.0.1:{port}/login");
    assert_eq!(fetch(&mut s, 1, &login, Some(&doc)), Ok(String::new()));
    let echo = format!("http://127.0.0.1:{port}/echo");
    assert_eq!(fetch(&mut s, 2, &echo, Some(&format!("http://127.0.0.1:{port}/other"))), Ok("theme=dark".into()));
    let forged = fetch(&mut s, 3, &echo, Some("https://bank.example.com/")).unwrap_err();
    assert_refused(&forged, 3);
    assert_eq!((netd.refused(None), netd.refused(Some(&token))), (1, 1));
}

#[test]
fn a_missing_initiator_stands_for_the_document_not_the_user() {
    let port = http_fixture();
    let mut netd = netd(ServerLimits::default());
    let login_doc = netd.token(&format!("http://127.0.0.1:{port}/"));
    let mut a = netd.content(&login_doc);
    assert!(fetch(&mut a, 1, &format!("http://127.0.0.1:{port}/login"), None).is_ok());
    let echo = format!("http://127.0.0.1:{port}/echo");
    assert_eq!(fetch(&mut a, 2, &echo, None), Ok("theme=dark".into()));
    // a document of another site gets the request as a cross-site one: the Lax cookie stays home
    let other = netd.token(&format!("http://localhost:{port}/"));
    let mut b = netd.content(&other);
    assert_eq!(fetch(&mut b, 3, &echo, None), Ok(String::new()));
}

#[test]
fn a_forged_cookie_url_gets_no_cookies_until_the_host_moves_the_document() {
    let port = http_fixture();
    let mut netd = netd(ServerLimits::default());
    let site = format!("http://127.0.0.1:{port}/");
    let a_token = netd.token(&site);
    let mut a = netd.content(&a_token);
    assert!(fetch(&mut a, 1, &format!("{site}login"), None).is_ok());
    assert_eq!(cookie_get(&mut a, &site).get_str(tag::TEXT), Some("theme=dark"));
    let b_token = netd.token(&format!("http://localhost:{port}/"));
    let mut b = netd.content(&b_token);
    assert_refused(&cookie_get(&mut b, &site), 9);
    Frame::new(kind::COOKIE_SET).num(tag::ID, 10).str(tag::URL, &site).str(tag::TEXT, "theme=light").write_to(&mut b).unwrap();
    assert_refused(&read(&mut b), 10);
    assert_eq!(cookie_get(&mut b, &format!("http://localhost:{port}/")).get_str(tag::TEXT), Some(""));
    assert_eq!(netd.refused(Some(&b_token)), 2);
    // the host commits a navigation of b's document to the other site
    assert_eq!(netd.register(&site, Some(&b_token)).kind, kind::REGISTERED);
    assert_eq!(cookie_get(&mut b, &site).get_str(tag::TEXT), Some("theme=dark"));
    assert_refused(&cookie_get(&mut b, &format!("http://localhost:{port}/")), 9);
    assert_eq!(netd.refused(None), 3);
}

#[test]
fn only_the_host_registers_and_only_registered_tokens_are_served() {
    let mut netd = netd(ServerLimits { clients: 2, ..ServerLimits::default() });
    let mut stranger = UnixStream::connect(&netd.path).unwrap();
    stranger.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    assert_refused(&cookie_get(&mut stranger, "http://127.0.0.1/"), 9);
    let mut stranger = UnixStream::connect(&netd.path).unwrap();
    stranger.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    Frame::new(kind::HELLO).num(tag::ID, 4).bytes(tag::TOKEN, &[7; 16]).write_to(&mut stranger).unwrap();
    assert_refused(&read(&mut stranger), 4);
    let token = netd.token("http://127.0.0.1/");
    let mut s = netd.content(&token);
    Frame::new(kind::REGISTER).num(tag::ID, 5).str(tag::URL, "https://bank.example.com/").write_to(&mut s).unwrap();
    assert_refused(&read(&mut s), 5);
    assert_eq!(netd.token("http://localhost/").len(), 16);
    assert_eq!(netd.register("http://a.test/", None).kind, kind::ERROR, "registrations are bounded");
    Frame::new(kind::UNREGISTER).num(tag::ID, 6).bytes(tag::TOKEN, &token).write_to(&mut netd.control).unwrap();
    assert_eq!(read(&mut netd.control).kind, kind::REGISTERED);
    assert_refused(&cookie_get(&mut s, "http://127.0.0.1/"), 9);
    assert_eq!(netd.refused(None), 4);
}

/// Whether the network process has closed `s`, within its read timeout.
fn closed(s: &mut UnixStream) -> bool {
    matches!(Frame::read_from(s), Ok(None))
}

#[test]
fn a_content_process_cannot_claim_a_navigation_or_raise_its_credentials() {
    let port = http_fixture();
    let mut netd = netd(ServerLimits::default());
    let site = netd.token(&format!("http://127.0.0.1:{port}/"));
    let mut a = netd.content(&site);
    assert!(fetch(&mut a, 1, &format!("http://127.0.0.1:{port}/login"), None).is_ok());
    let echo = format!("http://127.0.0.1:{port}/echo");
    // a cross-site navigation would carry the Lax cookie and hand the body to the document
    let other = netd.token(&format!("http://localhost:{port}/"));
    let mut b = netd.content(&other);
    let nav = Frame::new(kind::FETCH).num(tag::ID, 2).str(tag::URL, &echo).num(tag::NAVIGATION, 1);
    assert_refused(&exchange(&mut b, nav).unwrap_err(), 2);
    let hidden = Frame::new(kind::FETCH).num(tag::ID, 3).str(tag::URL, &echo).num(tag::NAVIGATION, 0).num(tag::NAVIGATION, 1);
    assert_refused(&exchange(&mut b, hidden).unwrap_err(), 3);
    // credentials may be lowered, never raised: of two values the lower one holds
    let creds = Frame::new(kind::FETCH).num(tag::ID, 4).str(tag::URL, &echo).num(tag::CREDENTIALS, 1).num(tag::CREDENTIALS, 0);
    assert_eq!(exchange(&mut a, creds), Ok(String::new()));
    let odd = Frame::new(kind::FETCH).num(tag::ID, 5).str(tag::URL, &echo).num(tag::CREDENTIALS, 3);
    assert_refused(&exchange(&mut a, odd).unwrap_err(), 5);
    assert_eq!(netd.refused(None), 3);
}

#[test]
fn a_connection_that_does_not_open_with_a_registered_hello_is_closed() {
    let mut netd = netd(ServerLimits { clients: 1, ..ServerLimits::default() });
    for _ in 0..3 {
        let mut stranger = UnixStream::connect(&netd.path).unwrap();
        stranger.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        assert_refused(&cookie_get(&mut stranger, "http://127.0.0.1/"), 9);
        assert!(closed(&mut stranger), "a client without a HELLO keeps its slot");
    }
    let mut guesser = UnixStream::connect(&netd.path).unwrap();
    guesser.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    Frame::new(kind::HELLO).num(tag::ID, 4).bytes(tag::TOKEN, &[7; 16]).write_to(&mut guesser).unwrap();
    assert_refused(&read(&mut guesser), 4);
    assert!(closed(&mut guesser), "a refused HELLO keeps its slot");
    let token = netd.token("http://127.0.0.1/");
    let mut s = netd.content(&token);
    Frame::new(kind::HELLO).num(tag::ID, 5).bytes(tag::TOKEN, &token).write_to(&mut s).unwrap();
    assert_refused(&read(&mut s), 5);
    assert!(closed(&mut s), "a second HELLO is refused and closes");
    assert_eq!(netd.refused(None), 5);
}

#[test]
fn a_malformed_token_is_refused_not_read_as_absent() {
    let mut netd = netd(ServerLimits::default());
    let token = netd.token("http://127.0.0.1/");
    let bad = netd.register("http://localhost/", Some(&token[..3]));
    assert_eq!((bad.kind, bad.get_str(tag::MESSAGE)), (kind::ERROR, Some("bad token")), "{bad:?}");
    Frame::new(kind::STATS).num(tag::ID, 2).bytes(tag::TOKEN, &[1; 17]).write_to(&mut netd.control).unwrap();
    let stats = read(&mut netd.control);
    assert_eq!((stats.kind, stats.get_str(tag::MESSAGE)), (kind::ERROR, Some("bad token")), "{stats:?}");
    Frame::new(kind::UNREGISTER).num(tag::ID, 3).bytes(tag::TOKEN, &[]).write_to(&mut netd.control).unwrap();
    let gone = read(&mut netd.control);
    assert_eq!((gone.kind, gone.get_str(tag::MESSAGE)), (kind::ERROR, Some("bad token")), "{gone:?}");
}

#[test]
fn the_binary_fails_when_its_control_channel_is_unusable() {
    let path = std::env::temp_dir().join(format!("mcguard-bin-{}.sock", std::process::id()));
    // descriptor 5 is open but no socket, so no host can ever register a document
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", "exec \"$0\" --socket \"$1\" --control-fd 5 5</dev/null", env!("CARGO_BIN_EXE_madcawl-netd"), path.to_str().unwrap()])
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break Some(st);
        }
        if start.elapsed() > Duration::from_secs(10) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = std::fs::remove_file(&path);
    assert!(status.is_some_and(|s| !s.success()), "{status:?}");
}

#[test]
fn the_host_fetches_navigations_on_its_own_connection() {
    let port = http_fixture();
    let mut netd = netd(ServerLimits::default());
    let site = netd.token(&format!("http://127.0.0.1:{port}/"));
    let mut a = netd.content(&site);
    assert!(fetch(&mut a, 1, &format!("http://127.0.0.1:{port}/login"), None).is_ok());
    Frame::new(kind::REGISTER).num(tag::ID, 2).num(tag::NAVIGATION, 1).write_to(&mut netd.control).unwrap();
    let host_token = read(&mut netd.control).field(tag::TOKEN).unwrap().to_vec();
    let mut host = netd.content(&host_token);
    // a link from another site's document: a cross-site top-level navigation, which carries Lax cookies
    let echo = format!("http://127.0.0.1:{port}/echo");
    let nav = Frame::new(kind::FETCH).num(tag::ID, 3).str(tag::URL, &echo).num(tag::NAVIGATION, 1).str(tag::INITIATOR, "http://localhost/");
    assert_eq!(exchange(&mut host, nav), Ok("theme=dark".into()));
    let sub = Frame::new(kind::FETCH).num(tag::ID, 4).str(tag::URL, &echo).num(tag::NAVIGATION, 0).str(tag::INITIATOR, "http://localhost/");
    assert_eq!(exchange(&mut host, sub), Ok(String::new()));
    let moved = netd.register(&echo, Some(&host_token));
    assert_eq!(moved.kind, kind::ERROR, "the host's registration is no document: {moved:?}");
    assert_eq!(netd.refused(None), 0);
}

#[test]
fn a_client_that_never_says_hello_is_dropped() {
    let mut netd = netd(ServerLimits { clients: 1, hello_timeout: Duration::from_millis(200), ..ServerLimits::default() });
    let mut silent = UnixStream::connect(&netd.path).unwrap();
    silent.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    assert!(closed(&mut silent), "a silent client keeps its slot");
    let token = netd.token("http://127.0.0.1/");
    let mut s = netd.content(&token);
    // once bound, a client may stay quiet for longer than the HELLO took
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(cookie_get(&mut s, "http://127.0.0.1/").kind, kind::COOKIES);
}

/// A stranger's connection that has sent the length of a frame and is
/// sending its body one byte at a time.
fn drip(path: &PathBuf, len: u32, every: Duration) -> UnixStream {
    let mut s = UnixStream::connect(path).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    s.write_all(&len.to_le_bytes()).unwrap();
    let mut w = s.try_clone().unwrap();
    std::thread::spawn(move || {
        for _ in 0..len {
            std::thread::sleep(every);
            if w.write_all(&[0]).is_err() {
                break;
            }
        }
    });
    s
}

#[test]
fn a_client_that_drips_its_hello_is_dropped_at_the_deadline() {
    let mut netd = netd(ServerLimits { clients: 1, hello_timeout: Duration::from_millis(300), ..ServerLimits::default() });
    let started = std::time::Instant::now();
    // each byte comes well within the timeout, the frame never does
    let mut slow = drip(&netd.path, 100, Duration::from_millis(50));
    assert!(closed(&mut slow), "a dripping client keeps its slot");
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    let token = netd.token("http://127.0.0.1/");
    let mut s = netd.content(&token);
    assert_eq!(cookie_get(&mut s, "http://127.0.0.1/").kind, kind::COOKIES);
}

#[test]
fn an_oversized_first_frame_is_refused_before_it_arrives() {
    let mut netd = netd(ServerLimits { clients: 1, ..ServerLimits::default() });
    // a frame far larger than any HELLO, of which only the length is sent
    let mut big = drip(&netd.path, 1 << 20, Duration::from_secs(60));
    assert!(closed(&mut big), "an unbound client may announce a large frame");
    let token = netd.token("http://127.0.0.1/");
    let mut s = netd.content(&token);
    assert_eq!(cookie_get(&mut s, "http://127.0.0.1/").kind, kind::COOKIES);
}

#[test]
fn a_connection_whose_document_is_unregistered_is_closed() {
    let mut netd = netd(ServerLimits { clients: 1, ..ServerLimits::default() });
    let token = netd.token("http://127.0.0.1/");
    let mut s = netd.content(&token);
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    Frame::new(kind::UNREGISTER).num(tag::ID, 6).bytes(tag::TOKEN, &token).write_to(&mut netd.control).unwrap();
    assert_eq!(read(&mut netd.control).kind, kind::REGISTERED);
    assert_refused(&cookie_get(&mut s, "http://127.0.0.1/"), 9);
    assert!(closed(&mut s), "a connection without a document keeps its slot");
    let token = netd.token("http://127.0.0.1/");
    let mut s = netd.content(&token);
    assert_eq!(cookie_get(&mut s, "http://127.0.0.1/").kind, kind::COOKIES);
}

#[test]
fn a_control_fd_that_is_not_open_is_refused_before_the_socket_is_bound() {
    let path = std::env::temp_dir().join(format!("mcguard-nofd-{}.sock", std::process::id()));
    // descriptor 3 is closed, so the listener would be given it
    let out = std::process::Command::new("/bin/sh")
        .args(["-c", "exec 3<&-; exec \"$0\" --socket \"$1\" --control-fd 3", env!("CARGO_BIN_EXE_madcawl-netd"), path.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{:?} {stderr}", out.status);
    assert!(stderr.contains("control"), "{stderr}");
}
