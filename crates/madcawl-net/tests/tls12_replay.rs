//! A captured TLS 1.2 connection to a server that offers no TLS 1.3,
//! replayed offline: with the recorder's randomness and clock the client
//! must send the captured bytes exactly, finish the handshake on TLS 1.2
//! and read the captured response.

mod capture {
    include!("capture/tls12_jal.rs");
}

use madcawl_net::NetError;
use madcawl_net::http1::{self, BodySink, RawHead, ReadLimits};
use madcawl_net::sock::{Bounds, Tcp};
use madcawl_net::stream::Stream;
use madcawl_net::tls::TlsStream;
use madcawl_net::trust::{Trust, TrustSource};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use torajs_tls::ProtocolVersion;
use torajs_tls::torajs_crypto::rand::{EntropyError, EntropySource};
use torajs_tls::torajs_x509::UnixTime;

/// The randomness `examples/record_tls12.rs` gave the client.
struct Fixed(u8);

impl EntropySource for Fixed {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), EntropyError> {
        for b in buf {
            self.0 = self.0.wrapping_mul(97).wrapping_add(13);
            *b = self.0;
        }
        Ok(())
    }
}

fn hex(s: &str) -> Vec<u8> {
    let digits: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    digits.chunks(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap()).collect()
}

type Flow = Vec<(char, Vec<u8>)>;

fn flow() -> Flow {
    capture::FLOW.iter().map(|(side, bytes)| (*side, hex(bytes))).collect()
}

/// Plays the server's side of `flow` to one connection, writing its bytes
/// `piece` at a time; fails where the client's bytes differ.
fn server(flow: Flow, piece: usize) -> (u16, JoinHandle<Result<(), String>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let played = std::thread::spawn(move || {
        let (mut s, _) = l.accept().map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        for (i, (side, bytes)) in flow.into_iter().enumerate() {
            if side == 's' {
                for p in bytes.chunks(piece) {
                    s.write_all(p).map_err(|e| format!("step {i}: {e}"))?;
                }
                continue;
            }
            let mut got = vec![0u8; bytes.len()];
            s.read_exact(&mut got).map_err(|e| format!("step {i}: {e}"))?;
            if got != bytes {
                let at = got.iter().zip(&bytes).position(|(a, b)| a != b).unwrap_or(0);
                return Err(format!("step {i}: the client's bytes differ from offset {at}"));
            }
        }
        Ok(())
    });
    (port, played)
}

struct Collect {
    head: Option<RawHead>,
    body: Vec<u8>,
}

impl BodySink for Collect {
    fn head(&mut self, head: &RawHead) -> Result<bool, NetError> {
        self.head = Some(head.clone());
        Ok(true)
    }

    fn body(&mut self, bytes: &[u8]) -> Result<(), NetError> {
        self.body.extend_from_slice(bytes);
        Ok(())
    }
}

/// The client's handshake against `flow`, checking chains at `now`, and
/// the thread playing the server.
fn handshake(flow: Flow, piece: usize, now: u64) -> (Result<TlsStream, NetError>, JoinHandle<Result<(), String>>) {
    let (port, played) = server(flow, piece);
    let sock = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let trust = Arc::new(Trust::new(TrustSource::Anchors(vec![hex(capture::ROOT)])));
    let tcp = Tcp::new(sock, Bounds::idle(Duration::from_secs(10)));
    let tls = TlsStream::handshake_with(tcp, capture::HOST, trust, &mut Fixed(1), UnixTime::from_secs(now));
    (tls, played)
}

/// The first flow item with the server's `ServerHello` rebuilt: `edit` gets
/// its random and its extensions as (type, data).
fn edit_server_hello(edit: impl FnOnce(&mut [u8], &mut Vec<(u16, Vec<u8>)>)) -> Flow {
    let mut flow = flow();
    let first = &mut flow[1].1;
    let record_len = 5 + usize::from(u16::from_be_bytes([first[3], first[4]]));
    let hello = &first[5..record_len];
    assert_eq!(hello[0], 2, "a ServerHello first");
    let body = &hello[4..];
    let mut random = body[2..34].to_vec();
    let sid = usize::from(body[34]);
    let fixed_end = 35 + sid + 3;
    let mut exts = Vec::new();
    let mut at = fixed_end + 2;
    while at < body.len() {
        let ty = u16::from_be_bytes([body[at], body[at + 1]]);
        let len = usize::from(u16::from_be_bytes([body[at + 2], body[at + 3]]));
        exts.push((ty, body[at + 4..at + 4 + len].to_vec()));
        at += 4 + len;
    }
    edit(&mut random, &mut exts);
    let mut ext_bytes = Vec::new();
    for (ty, data) in &exts {
        ext_bytes.extend_from_slice(&ty.to_be_bytes());
        ext_bytes.extend_from_slice(&(data.len() as u16).to_be_bytes());
        ext_bytes.extend_from_slice(data);
    }
    let mut new_body = body[..2].to_vec();
    new_body.extend_from_slice(&random);
    new_body.extend_from_slice(&body[34..fixed_end]);
    new_body.extend_from_slice(&(ext_bytes.len() as u16).to_be_bytes());
    new_body.extend_from_slice(&ext_bytes);
    let mut new_hello = vec![2];
    new_hello.extend_from_slice(&(new_body.len() as u32).to_be_bytes()[1..]);
    new_hello.extend_from_slice(&new_body);
    let mut rebuilt = first[..3].to_vec();
    rebuilt.extend_from_slice(&(new_hello.len() as u16).to_be_bytes());
    rebuilt.extend_from_slice(&new_hello);
    rebuilt.extend_from_slice(&first[record_len..]);
    *first = rebuilt;
    flow
}

fn replay(piece: usize) {
    let (tls, played) = handshake(flow(), piece, capture::NOW);
    let tls = match tls {
        Ok(t) => t,
        Err(e) => panic!("{e}; the server side: {:?}", played.join().unwrap()),
    };
    assert_eq!(tls.protocol_version(), Some(ProtocolVersion::Tls12));
    let mut stream = Stream::Tls(Box::new(tls));
    let headers: [(&str, &[u8]); 3] = [
        ("Host", capture::HOST.as_bytes()),
        ("User-Agent", b"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) madcawl/0.0.1"),
        ("Accept-Encoding", b"identity"),
    ];
    http1::write_request(&mut stream, "GET", capture::PATH, &headers, None).unwrap();
    let mut sink = Collect { head: None, body: Vec::new() };
    http1::read_response(&mut stream, "GET", &ReadLimits { max_body: 1 << 20 }, &mut sink).unwrap();
    if let Stream::Tls(t) = &mut stream {
        t.close();
    }
    played.join().unwrap().unwrap();
    assert_eq!(sink.head.unwrap().status, capture::STATUS);
    assert_eq!(sink.body, hex(capture::BODY));
}

#[test]
fn a_captured_tls12_connection_replays_byte_for_byte() {
    replay(usize::MAX);
}

#[test]
fn a_captured_tls12_connection_replays_with_the_server_bytes_one_at_a_time() {
    replay(1);
}

#[test]
fn the_captured_chain_is_refused_once_the_clock_is_past_the_leaf() {
    let (r, _played) = handshake(flow(), usize::MAX, capture::NOW + 20 * 366 * 86_400);
    assert!(matches!(r, Err(NetError::Untrusted(_))), "{:?}", r.err());
}

#[test]
fn the_unedited_server_hello_rebuilds_to_the_captured_bytes() {
    assert_eq!(edit_server_hello(|_, _| {}), flow());
}

// the extended master secret is required by policy here; RFC 7627 §5.2
// only says a client should abort without it
#[test]
fn a_server_hello_without_the_extended_master_secret_is_refused() {
    let flow = edit_server_hello(|_, exts| exts.retain(|(ty, _)| *ty != 0x0017));
    let (r, _played) = handshake(flow, usize::MAX, capture::NOW);
    match r {
        Err(NetError::Tls(m)) => assert!(m.contains("ExtendedMasterSecretRequired"), "{m}"),
        r => panic!("{:?}", r.err()),
    }
}

#[test]
fn a_server_hello_with_the_tls13_downgrade_marker_is_refused() {
    let flow = edit_server_hello(|random, _| random[24..].copy_from_slice(b"DOWNGRD\x01"));
    let (r, _played) = handshake(flow, usize::MAX, capture::NOW);
    match r {
        Err(NetError::Tls(m)) => assert!(m.contains("Downgrade"), "{m}"),
        r => panic!("{:?}", r.err()),
    }
}
