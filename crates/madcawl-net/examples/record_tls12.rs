//! Records one TLS 1.2 handshake and one HTTP/1.1 request of this stack
//! against a live server, for the capture `tests/tls12_replay.rs` replays.
//!
//! ```text
//! cargo run -p madcawl-net --example record_tls12 -- <host> <path> <root.der>
//! ```
//!
//! The client goes to `host` port 443 through a local relay that keeps the
//! bytes of both directions in the order they pass, draws its randomness
//! from the fixed source the replay uses, and checks the chain against
//! `root.der` alone at the current time. It sends `GET <path>`, reads the
//! response and sends `close_notify`, then prints the capture as Rust
//! source on stdout.
//!
//! The randomness is fixed and public, so anyone holding a recorded flow
//! can derive its session keys and read it. Record only public resources,
//! never anything behind a login or that sets cookies.

use madcawl_net::http1::{self, BodySink, RawHead, ReadLimits};
use madcawl_net::sock::{Bounds, Tcp};
use madcawl_net::stream::Stream;
use madcawl_net::tls::TlsStream;
use madcawl_net::trust::{Trust, TrustSource};
use madcawl_net::{NetError, fetch};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use torajs_tls::torajs_crypto::rand::{EntropyError, EntropySource};
use torajs_tls::torajs_x509::UnixTime;

const TIMEOUT: Duration = Duration::from_secs(10);

/// The randomness the replay gives the client.
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

type Log = Arc<Mutex<Vec<(char, Vec<u8>)>>>;

/// Copies `from` to `to`, logging each piece as `side` before passing it
/// on, so a reply is always logged after what caused it.
fn pump(mut from: TcpStream, mut to: TcpStream, side: char, log: Log) {
    let mut buf = [0u8; 16 * 1024];
    while let Ok(n) = from.read(&mut buf) {
        if n == 0 {
            break;
        }
        log.lock().unwrap().push((side, buf[..n].to_vec()));
        if to.write_all(&buf[..n]).is_err() {
            break;
        }
    }
    let _ = to.shutdown(Shutdown::Write);
}

/// A relay on a local port to `upstream`, for one connection; the thread
/// returned ends once the client's side has ended.
fn relay(upstream: TcpStream, log: Log) -> Result<(u16, JoinHandle<()>), String> {
    let l = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = l.local_addr().map_err(|e| e.to_string())?.port();
    let client_side = std::thread::spawn(move || {
        let Ok((client, _)) = l.accept() else { return };
        let (c2, u2) = (client.try_clone().unwrap(), upstream.try_clone().unwrap());
        let l2 = log.clone();
        std::thread::spawn(move || pump(u2, c2, 's', l2));
        pump(client, upstream, 'c', log);
    });
    Ok((port, client_side))
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [host, path, root] = &args[..] else {
        return Err("usage: record_tls12 <host> <path> <root.der>".into());
    };
    let root = std::fs::read(root).map_err(|e| e.to_string())?;
    let addr = (host.as_str(), 443).to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or("no address")?;
    let upstream = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| e.to_string())?;
    let log: Log = Arc::default();
    let (port, client_side) = relay(upstream, log.clone())?;
    let sock = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    let now = fetch::now_secs() as u64;
    let trust = Arc::new(Trust::new(TrustSource::Anchors(vec![root.clone()])));
    let tcp = Tcp::new(sock, Bounds::idle(TIMEOUT));
    let tls = TlsStream::handshake_with(tcp, host, trust, &mut Fixed(1), UnixTime::from_secs(now));
    let tls = tls.map_err(|e| e.to_string())?;
    let version = tls.protocol_version().map(|v| v.to_string()).unwrap_or_default();
    let mut stream = Stream::Tls(Box::new(tls));
    let headers: [(&str, &[u8]); 3] = [
        ("Host", host.as_bytes()),
        ("User-Agent", b"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) madcawl/0.0.1"),
        ("Accept-Encoding", b"identity"),
    ];
    http1::write_request(&mut stream, "GET", path, &headers, None).map_err(|e| e.to_string())?;
    let mut sink = Collect { head: None, body: Vec::new() };
    let limits = ReadLimits { max_body: 1 << 20 };
    http1::read_response(&mut stream, "GET", &limits, &mut sink).map_err(|e| e.error.to_string())?;
    if let Stream::Tls(t) = &mut stream {
        t.close();
    }
    stream.tcp().shutdown(Shutdown::Write).map_err(|e| e.to_string())?;
    client_side.join().map_err(|_| "the relay failed")?;
    let mut flow = std::mem::take(&mut *log.lock().unwrap());
    // what the server sent after the client's last bytes is no part of
    // the exchange the replay checks
    while flow.last().is_some_and(|(side, _)| *side == 's') {
        flow.pop();
    }
    let head = sink.head.ok_or("no response head")?;
    let mut merged: Vec<(char, Vec<u8>)> = Vec::new();
    for (side, bytes) in flow {
        match merged.last_mut() {
            Some((s, b)) if *s == side => b.extend_from_slice(&bytes),
            _ => merged.push((side, bytes)),
        }
    }
    println!("// {version}, {} {}, recorded at unix time {now}", head.status, head.reason);
    println!("pub const HOST: &str = {host:?};");
    println!("pub const PATH: &str = {path:?};");
    println!("pub const NOW: u64 = {now};");
    println!("pub const ROOT: &str = \"{}\";", hex(&root));
    println!("pub const STATUS: u16 = {};", head.status);
    println!("pub const BODY: &str = \"{}\";", hex(&sink.body));
    println!("pub const FLOW: &[(char, &str)] = &[");
    for (side, bytes) in merged {
        println!("    ('{side}', \"{}\"),", hex(&bytes));
    }
    println!("];");
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("record_tls12: {why}");
            ExitCode::FAILURE
        }
    }
}
