//! HTTP/1.1 behaviour against a local fixture server: framing, keep-alive,
//! redirects, content decoding and its cap, cookies, timeouts and TLS.

mod common;

use common::{CA, Req, reply, serve, serve_tls};
use madcawl_net::fetch::{Config, Redirect, Request};
use madcawl_net::pool::PoolLimits;
use madcawl_net::trust::TrustSource;
use madcawl_net::{Client, NetError, Url};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use torajs_flate::cpu::CpuFeatures;
use torajs_flate::deflate::{Deflate, Level};
use torajs_flate::{Flush, Format, Status};

fn client() -> Client {
    Client::new(Config { io_timeout: Duration::from_secs(5), ..Config::default() })
}

fn get(c: &Client, url: &str) -> Result<madcawl_net::Response, NetError> {
    c.fetch(&Request::get(Url::parse(url).unwrap()))
}

fn compress(data: &[u8], format: Format) -> Vec<u8> {
    compress_at(data, format, Level::DEFAULT)
}

fn compress_at(data: &[u8], format: Format, level: Level) -> Vec<u8> {
    let mut mem = vec![0u64; Deflate::SIZE.div_ceil(8)];
    // SAFETY: a u64 buffer viewed as bytes of the same length
    let bytes = unsafe { std::slice::from_raw_parts_mut(mem.as_mut_ptr().cast::<u8>(), mem.len() * 8) };
    // SAFETY: zeroed, 8-byte aligned and SIZE bytes long
    let e = unsafe { Deflate::from_zeroed(bytes) };
    e.reset(format, level, CpuFeatures::compile_time());
    let mut out = Vec::new();
    let mut chunk = vec![0u8; 1 << 16];
    let mut at = 0;
    loop {
        let p = e.deflate(&data[at..], &mut chunk, Flush::Finish).unwrap();
        at += p.consumed;
        out.extend_from_slice(&chunk[..p.produced]);
        if p.status == Status::Done {
            return out;
        }
    }
}

#[test]
fn content_length_and_chunked_bodies() {
    let f = serve(Arc::new(|r: &Req| {
        Some(match r.target.as_str() {
            "/chunked" => b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5;ext=1\r\nhello\r\n6\r\n world\r\n0\r\nT: x\r\n\r\n".to_vec(),
            _ => reply(200, &[("Content-Type", "text/plain")], b"plain body"),
        })
    }));
    let c = client();
    let r = get(&c, &f.url("/a?q=1#frag")).unwrap();
    assert_eq!((r.status, r.body.as_slice()), (200, &b"plain body"[..]));
    assert_eq!(r.header("content-type"), Some(&b"text/plain"[..]));
    assert_eq!(get(&c, &f.url("/chunked")).unwrap().body, b"hello world");
    let reqs = f.requests();
    assert_eq!(reqs[0].target, "/a?q=1");
    assert_eq!(reqs[0].header("host").unwrap(), f.addr.to_string());
    assert_eq!(reqs[0].header("accept-encoding").unwrap(), "gzip, deflate");
    assert_eq!(f.connections(), 1, "the second request reused the connection");
}

#[test]
fn keep_alive_reuse_close_and_stale_connections() {
    let f = serve(Arc::new(|r: &Req| match r.target.as_str() {
        "/close" => Some(reply(200, &[("Connection", "close")], b"bye")),
        // the server drops a kept-alive connection on its second request
        "/flaky" if r.conn == 3 => None,
        _ => Some(reply(200, &[], b"ok")),
    }));
    let c = client();
    for _ in 0..3 {
        get(&c, &f.url("/")).unwrap();
    }
    assert_eq!(f.connections(), 1);
    get(&c, &f.url("/close")).unwrap();
    assert_eq!(c.pool().idle_count(), 0);
    get(&c, &f.url("/")).unwrap();
    assert_eq!(f.connections(), 2);
    get(&c, &f.url("/close")).unwrap();
    get(&c, &f.url("/")).unwrap();
    assert_eq!(f.connections(), 3);
    let r = get(&c, &f.url("/flaky")).unwrap();
    assert_eq!(r.body, b"ok", "retried on a new connection");
    assert_eq!(f.connections(), 4);
}

#[test]
fn the_pool_keeps_within_its_caps() {
    let f = serve(Arc::new(|_: &Req| Some(reply(200, &[], b"ok"))));
    let (now, peak) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (n2, p2) = (now.clone(), peak.clone());
    let g = serve(Arc::new(move |_: &Req| {
        let k = n2.fetch_add(1, Ordering::SeqCst) + 1;
        p2.fetch_max(k, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(30));
        n2.fetch_sub(1, Ordering::SeqCst);
        Some(reply(200, &[], b"ok"))
    }));
    let c = Client::new(Config {
        pool: PoolLimits { per_origin: 2, max_idle: 1, idle_timeout: Duration::from_secs(60) },
        ..Config::default()
    });
    get(&c, &f.url("/")).unwrap();
    get(&c, &g.url("/")).unwrap();
    assert_eq!(c.pool().idle_count(), 1);
    get(&c, &f.url("/")).unwrap();
    assert_eq!(f.connections(), 2, "the idle connection to f was evicted for g's");
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| get(&c, &g.url("/")).unwrap());
        }
    });
    assert_eq!(peak.load(Ordering::SeqCst), 2, "two requests at a time to one origin, no more");
}

#[test]
fn redirects_rewrite_methods_and_stop_at_the_cap() {
    let f = serve(Arc::new(|r: &Req| {
        Some(match r.target.as_str() {
            "/302" => reply(302, &[("Location", "/dest")], b""),
            "/303" => reply(303, &[("Location", "dest")], b""),
            "/307" => reply(307, &[("Location", "/dest")], b""),
            "/loop" => reply(301, &[("Location", "/loop")], b""),
            "/away" => reply(302, &[("Location", "ftp://example.invalid/")], b""),
            _ => reply(200, &[], format!("{} {}", r.method, r.body.len()).as_bytes()),
        })
    }));
    let c = client();
    let post = |path: &str| {
        let mut req = Request::get(Url::parse(&f.url(path)).unwrap());
        req.method = "POST".into();
        req.body = Some(b"a=1".to_vec());
        req.headers.push(("Content-Type".into(), b"application/x-www-form-urlencoded".to_vec()));
        c.fetch(&req)
    };
    let r = post("/302").unwrap();
    assert_eq!((r.body.as_slice(), r.url().path()), (&b"GET 0"[..], "/dest"));
    assert_eq!(r.url_list.len(), 2);
    assert_eq!(post("/303").unwrap().body, b"GET 0");
    assert_eq!(post("/307").unwrap().body, b"POST 3");
    let last = f.requests().pop().unwrap();
    assert_eq!(last.header("content-type").as_deref(), Some("application/x-www-form-urlencoded"));
    let n = f.requests().len();
    let capped = Client::new(Config { max_redirects: 5, ..Config::default() });
    assert_eq!(get(&capped, &f.url("/loop")), Err(NetError::TooManyRedirects));
    assert_eq!(f.requests().len() - n, 6);
    assert!(matches!(get(&c, &f.url("/away")), Err(NetError::Scheme(_))));
    let mut manual = Request::get(Url::parse(&f.url("/302")).unwrap());
    manual.redirect = Redirect::Manual;
    assert_eq!(c.fetch(&manual).unwrap().status, 302);
}

#[test]
fn gzip_and_deflate_and_the_bomb_cap() {
    let text = b"the quick brown fox ".repeat(500);
    let gz = compress(&text, Format::Gzip);
    let zl = compress(&text, Format::Zlib);
    let raw = compress(&text, Format::Raw);
    let bomb = compress(&vec![0u8; 8 << 20], Format::Gzip);
    assert!(bomb.len() < 64 << 10);
    let f = serve(Arc::new(move |r: &Req| {
        Some(match r.target.as_str() {
            "/gz" => reply(200, &[("Content-Encoding", "gzip")], &gz),
            "/zlib" => reply(200, &[("Content-Encoding", "deflate")], &zl),
            "/raw" => reply(200, &[("Content-Encoding", "deflate")], &raw),
            "/bomb" => reply(200, &[("Content-Encoding", "gzip")], &bomb),
            "/br" => reply(200, &[("Content-Encoding", "br")], b"x"),
            _ => reply(200, &[], &vec![b'x'; 64 << 10]),
        })
    }));
    let c = Client::new(Config { max_decoded: 1 << 20, max_body: 32 << 10, ..Config::default() });
    for p in ["/gz", "/zlib", "/raw"] {
        assert_eq!(get(&c, &f.url(p)).unwrap().body, text, "{p}");
    }
    assert_eq!(get(&c, &f.url("/bomb")), Err(NetError::TooLarge("decoded body")));
    assert!(matches!(get(&c, &f.url("/br")), Err(NetError::Decode(_))));
    assert_eq!(get(&c, &f.url("/big")), Err(NetError::TooLarge("response body")));
}

#[test]
fn every_layer_of_nested_codings_is_capped() {
    use madcawl_net::decode::{Coding, decode};
    // an inner gzip stream exactly one 32 KiB piece of the outer layer's
    // output long, so it ends at a piece boundary; the outer layer then
    // inflates megabytes of zeros the inner one never reads
    let stored = |n: usize| compress_at(&vec![b's'; n], Format::Gzip, Level::NONE);
    let overhead = stored(1000).len() - 1000;
    let mut inner = ((32 << 10) - overhead - 64..=(32 << 10) - overhead)
        .rev()
        .map(stored)
        .find(|s| s.len() == 32 << 10)
        .expect("a stored stream of exactly 32 KiB");
    inner.resize(inner.len() + (16 << 20), 0);
    let body = compress(&inner, Format::Gzip);
    assert!(body.len() < 64 << 10);
    let started = std::time::Instant::now();
    let r = decode(&body, &[Coding::Gzip, Coding::Gzip], 1 << 20);
    assert!(matches!(r, Err(NetError::TooLarge(_) | NetError::Decode(_))), "{:?}", r.map(|b| b.len()));
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    // nor may an outer layer alone inflate past the cap
    let deep = compress(&compress(&vec![0u8; 4 << 20], Format::Gzip), Format::Gzip);
    assert_eq!(decode(&deep, &[Coding::Gzip, Coding::Gzip], 1 << 20), Err(NetError::TooLarge("decoded body")));
    let fine = compress(&compress(b"hello", Format::Gzip), Format::Gzip);
    assert_eq!(decode(&fine, &[Coding::Gzip, Coding::Gzip], 1 << 20).unwrap(), b"hello");
}

#[test]
fn cookies_are_stored_sent_and_follow_redirects() {
    let f = serve(Arc::new(|r: &Req| {
        Some(match r.target.as_str() {
            "/login" => reply(
                302,
                &[("Location", "/home"), ("Set-Cookie", "sid=s3cret; HttpOnly; Path=/"), ("Set-Cookie", "theme=dark")],
                b"",
            ),
            "/logout" => reply(200, &[("Set-Cookie", "sid=; Max-Age=0; Path=/")], b""),
            _ => reply(200, &[], r.header("cookie").unwrap_or_default().as_bytes()),
        })
    }));
    let c = client();
    let r = get(&c, &f.url("/login")).unwrap();
    assert_eq!(r.body, b"sid=s3cret; theme=dark");
    get(&c, &f.url("/logout")).unwrap();
    assert_eq!(get(&c, &f.url("/home")).unwrap().body, b"theme=dark");
    let mut anon = Request::get(Url::parse(&f.url("/home")).unwrap());
    anon.credentials = false;
    assert_eq!(c.fetch(&anon).unwrap().body, b"");
    let mut forged = Request::get(Url::parse(&f.url("/home")).unwrap());
    forged.headers.push(("Cookie".into(), b"sid=forged".to_vec()));
    assert_eq!(c.fetch(&forged).unwrap().body, b"theme=dark", "a requester cannot set Cookie");
}

#[test]
fn timeouts_and_refused_connections() {
    let f = serve(Arc::new(|_: &Req| {
        std::thread::sleep(Duration::from_secs(3));
        Some(reply(200, &[], b"late"))
    }));
    let c = Client::new(Config { io_timeout: Duration::from_millis(300), ..Config::default() });
    assert_eq!(get(&c, &f.url("/")), Err(NetError::ReadTimeout));
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    assert!(matches!(get(&c, &format!("http://127.0.0.1:{port}/")), Err(NetError::Connect(_))));
}

#[test]
fn tls_with_a_local_ca_and_dns_for_localhost() {
    let f = serve_tls(Arc::new(|r: &Req| Some(reply(200, &[], format!("tls {}", r.target).as_bytes()))));
    let c = Client::new(Config { trust: TrustSource::Anchors(vec![CA.to_vec()]), ..Config::default() });
    assert_eq!(get(&c, &f.https("/a")).unwrap().body, b"tls /a");
    assert_eq!(get(&c, &f.https("/b")).unwrap().body, b"tls /b");
    assert_eq!(f.connections(), 1, "kept alive over TLS");
    assert_eq!(c.dns().len(), 1, "localhost went through dnssd and the cache");
    let ip = format!("https://127.0.0.1:{}/ip", f.addr.port());
    assert_eq!(get(&c, &ip).unwrap().body, b"tls /ip");
    let system = client();
    assert!(matches!(get(&system, &f.https("/")), Err(NetError::Untrusted(_))), "the test CA is not a system root");
}
