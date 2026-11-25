//! What a requester may not do and the bounds every request runs within:
//! forbidden methods and method overrides, certificate names, interim
//! responses, the whole-request deadline, a poisoned cookie jar, and a
//! body handed on as it arrives.

mod common;

use common::{CA, Req, reply, serve, serve_tls, serve_tls_at, serve_tls_dribbling};
use madcawl_net::fetch::{Config, Request, Sink};
use madcawl_net::trust::TrustSource;
use madcawl_net::{Client, NetError, Response, Url};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn client() -> Client {
    Client::new(Config { io_timeout: Duration::from_secs(5), ..Config::default() })
}

fn request(method: &str, url: &str) -> Request {
    let mut r = Request::get(Url::parse(url).unwrap());
    r.method = method.into();
    r
}

#[test]
fn forbidden_methods_never_reach_the_server() {
    let f = serve(Arc::new(|r: &Req| Some(reply(200, &[], r.method.as_bytes()))));
    let c = client();
    for m in ["TRACE", "trace", "Track", "CONNECT"] {
        assert!(matches!(c.fetch(&request(m, &f.url("/"))), Err(NetError::Refused(_))), "{m}");
    }
    assert!(f.requests().is_empty());
    assert_eq!(c.fetch(&request("post", &f.url("/"))).unwrap().body, b"POST");
    assert_eq!(c.fetch(&request("patch", &f.url("/"))).unwrap().body, b"patch", "only the six are upper-cased");
    let mut r = request("GET", &f.url("/o"));
    r.headers.push(("X-HTTP-Method-Override".into(), b"trace".to_vec()));
    r.headers.push(("X-Method-Override".into(), b"PATCH".to_vec()));
    c.fetch(&r).unwrap();
    let seen = f.requests().pop().unwrap();
    assert_eq!(seen.header("x-http-method-override"), None);
    assert_eq!(seen.header("x-method-override").as_deref(), Some("PATCH"));
}

#[test]
fn a_certificate_for_another_name_is_rejected() {
    // the test certificate names localhost and 127.0.0.1, not ::1
    let f = serve_tls_at("[::1]:0", Arc::new(|_: &Req| Some(reply(200, &[], b"no"))));
    let c = Client::new(Config { trust: TrustSource::Anchors(vec![CA.to_vec()]), ..Config::default() });
    let url = format!("https://[::1]:{}/", f.addr.port());
    match c.fetch(&Request::get(Url::parse(&url).unwrap())) {
        Err(NetError::Untrusted(m)) => assert!(m.contains("Name"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(f.requests().is_empty());
}

#[test]
fn a_system_rejection_says_why() {
    let f = serve_tls(Arc::new(|_: &Req| Some(reply(200, &[], b"no"))));
    match client().fetch(&Request::get(Url::parse(&f.https("/")).unwrap())) {
        // the system's own reason, not only the alert sent to the server
        Err(NetError::Untrusted(m)) => assert!(m.contains("localhost") && m.contains("BadCertificate"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn interim_responses_are_capped() {
    let f = serve(Arc::new(|r: &Req| {
        let n: usize = r.target[1..].parse().unwrap();
        let mut out = b"HTTP/1.1 103 Early Hints\r\nLink: </a>\r\n\r\n".repeat(n);
        out.extend_from_slice(&reply(200, &[], b"done"));
        Some(out)
    }));
    let c = client();
    assert_eq!(c.fetch(&request("GET", &f.url("/16"))).unwrap().body, b"done");
    assert!(matches!(c.fetch(&request("GET", &f.url("/17"))), Err(NetError::Http(_))));
}

/// A server that sends a head and then one body byte every 100 ms.
fn dribbler() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            std::thread::spawn(move || {
                let mut b = [0u8; 4096];
                let _ = s.read(&mut b);
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 40\r\n\r\n");
                for _ in 0..40 {
                    std::thread::sleep(Duration::from_millis(100));
                    if s.write_all(b"x").is_err() {
                        return;
                    }
                }
            });
        }
    });
    format!("http://{addr}/")
}

#[test]
fn a_slow_server_hits_the_request_deadline() {
    let url = dribbler();
    let c = Client::new(Config {
        io_timeout: Duration::from_secs(2),
        request_timeout: Duration::from_millis(600),
        ..Config::default()
    });
    let start = Instant::now();
    assert_eq!(c.fetch(&request("GET", &url)), Err(NetError::Deadline));
    assert!(start.elapsed() < Duration::from_millis(1500), "{:?}", start.elapsed());
}

#[test]
fn a_tls_server_sending_a_byte_at_a_time_hits_the_request_deadline() {
    // each byte comes well within the per-read timeout, but one record of
    // the reply takes seconds to arrive
    let f = serve_tls_dribbling(Arc::new(|_: &Req| Some(reply(200, &[], &[b'x'; 200]))), Duration::from_millis(20));
    let c = Client::new(Config {
        trust: TrustSource::Anchors(vec![CA.to_vec()]),
        io_timeout: Duration::from_secs(2),
        request_timeout: Duration::from_millis(600),
        ..Config::default()
    });
    let start = Instant::now();
    assert_eq!(c.fetch(&request("GET", &f.https("/"))), Err(NetError::Deadline));
    assert!(start.elapsed() < Duration::from_millis(1500), "{:?}", start.elapsed());
}

#[test]
fn an_upload_the_server_never_reads_hits_the_request_deadline() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    std::thread::spawn(move || {
        let (s, _) = l.accept().unwrap();
        std::thread::sleep(Duration::from_secs(10));
        drop(s);
    });
    let c = Client::new(Config {
        io_timeout: Duration::from_secs(3),
        request_timeout: Duration::from_millis(500),
        ..Config::default()
    });
    let mut r = request("POST", &url);
    r.body = Some(vec![b'u'; 16 << 20]);
    let start = Instant::now();
    assert_eq!(c.fetch(&r), Err(NetError::Deadline));
    assert!(start.elapsed() < Duration::from_millis(1500), "{:?}", start.elapsed());
}

#[test]
fn a_poisoned_cookie_jar_is_taken_over() {
    let f = serve(Arc::new(|r: &Req| {
        Some(match r.target.as_str() {
            "/set" => reply(200, &[("Set-Cookie", "a=1")], b""),
            _ => reply(200, &[], r.header("cookie").unwrap_or_default().as_bytes()),
        })
    }));
    let c = Arc::new(client());
    let c2 = c.clone();
    let poisoner = std::thread::spawn(move || {
        let _jar = c2.lock_jar();
        panic!("poison the jar");
    });
    assert!(poisoner.join().is_err());
    assert!(c.jar().is_poisoned());
    c.fetch(&request("GET", &f.url("/set"))).unwrap();
    assert_eq!(c.fetch(&request("GET", &f.url("/get"))).unwrap().body, b"a=1");
}

#[test]
fn the_body_is_handed_on_as_it_arrives() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut b = [0u8; 4096];
        let _ = s.read(&mut b);
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nfirst");
        std::thread::sleep(Duration::from_millis(800));
        let _ = s.write_all(b"later");
    });
    struct Timed {
        start: Instant,
        head: Option<Duration>,
        chunks: Vec<(Duration, Vec<u8>)>,
    }
    impl Sink for Timed {
        fn head(&mut self, _: &Response) -> Result<(), NetError> {
            self.head = Some(self.start.elapsed());
            Ok(())
        }
        fn body(&mut self, chunk: &[u8]) -> Result<(), NetError> {
            self.chunks.push((self.start.elapsed(), chunk.to_vec()));
            Ok(())
        }
    }
    let mut t = Timed { start: Instant::now(), head: None, chunks: Vec::new() };
    client().fetch_into(&request("GET", &url), &mut t).unwrap();
    assert!(t.head.unwrap() < Duration::from_millis(500));
    assert_eq!(t.chunks[0].1, b"first");
    assert!(t.chunks[0].0 < Duration::from_millis(500), "the first piece went on before the rest came");
    let body: Vec<u8> = t.chunks.iter().flat_map(|(_, b)| b.iter().copied()).collect();
    assert_eq!(body, b"firstlater");
}
