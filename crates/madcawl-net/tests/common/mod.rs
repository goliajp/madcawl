//! A local HTTP/1.1 fixture server, plain or over TLS 1.3 with a test CA.
//! Each connection runs on its own thread and serves requests until the
//! client closes it or the reply says to close.

#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use torajs_http1::{BodyDecoder, Limits, Progress, RequestHead};
use torajs_tls::server::{Connection, RECOMMENDED_BUFFER_LEN, ServerConfig, SigningKey};
use torajs_tls::torajs_crypto::cpu::CpuFeatures;

mod certs;

pub use certs::CA;
use certs::{KEY, LEAF};

#[derive(Clone, Debug)]
pub struct Req {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
    /// which accepted connection carried it, from 1
    pub conn: usize,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
    }
}

/// The bytes to answer with; `None` closes the connection without a word.
pub type Handler = Arc<dyn Fn(&Req) -> Option<Vec<u8>> + Send + Sync>;

pub struct Fixture {
    pub addr: SocketAddr,
    pub seen: Arc<Mutex<Vec<Req>>>,
    pub accepted: Arc<AtomicUsize>,
}

impl Fixture {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    pub fn https(&self, path: &str) -> String {
        format!("https://localhost:{}{path}", self.addr.port())
    }

    pub fn requests(&self) -> Vec<Req> {
        self.seen.lock().unwrap().clone()
    }

    pub fn connections(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
}

pub fn reply(status: u16, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\n", body.len()).into_bytes();
    for (n, v) in headers {
        out.extend_from_slice(format!("{n}: {v}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

pub fn serve(handler: Handler) -> Fixture {
    start("127.0.0.1:0", handler, false, None)
}

pub fn serve_tls(handler: Handler) -> Fixture {
    start("127.0.0.1:0", handler, true, None)
}

/// TLS on `bind`, which the certificate may not name.
pub fn serve_tls_at(bind: &str, handler: Handler) -> Fixture {
    start(bind, handler, true, None)
}

/// TLS that, after the handshake, sends its records one byte every
/// `delay`.
pub fn serve_tls_dribbling(handler: Handler, delay: Duration) -> Fixture {
    start("127.0.0.1:0", handler, true, Some(delay))
}

fn start(bind: &str, handler: Handler, tls: bool, dribble: Option<Duration>) -> Fixture {
    let listener = TcpListener::bind(bind).unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let accepted = Arc::new(AtomicUsize::new(0));
    let (s, a) = (seen.clone(), accepted.clone());
    std::thread::spawn(move || {
        for tcp in listener.incoming() {
            let Ok(tcp) = tcp else { continue };
            let n = a.fetch_add(1, Ordering::SeqCst) + 1;
            let (h, s) = (handler.clone(), s.clone());
            std::thread::spawn(move || {
                tcp.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
                if tls {
                    let _ = serve_tls_conn(tcp, n, &h, &s, dribble);
                } else {
                    let mut tcp = tcp;
                    let _ = serve_conn(&mut tcp, n, &h, &s);
                }
            });
        }
    });
    Fixture { addr, seen, accepted }
}

fn serve_conn(io: &mut (impl Read + Write), n: usize, h: &Handler, seen: &Mutex<Vec<Req>>) -> io::Result<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let (head, used) = loop {
            if let Ok(Progress::Complete { consumed, value }) = RequestHead::parse(&buf, &Limits::new()) {
                break (value, consumed);
            }
            let k = io.read(&mut chunk)?;
            if k == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&chunk[..k]);
        };
        buf.drain(..used);
        let mut dec = BodyDecoder::new(head.body_kind());
        let mut body = Vec::new();
        loop {
            match dec.decode(&buf, &mut body).map_err(io::Error::other)? {
                Progress::Complete { consumed, .. } => {
                    buf.drain(..consumed);
                    break;
                }
                Progress::Incomplete => buf.clear(),
            }
            let k = io.read(&mut chunk)?;
            if k == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&chunk[..k]);
        }
        let req = Req {
            method: head.method().to_owned(),
            target: head.target().to_owned(),
            headers: head.headers().iter().map(|h| (h.name().to_owned(), h.value().to_vec())).collect(),
            body,
            conn: n,
        };
        seen.lock().unwrap().push(req.clone());
        let Some(out) = h(&req) else { return Ok(()) };
        io.write_all(&out)?;
        io.flush()?;
    }
}

struct ServerTls<'c, 'b> {
    conn: Connection<'c, 'b>,
    tcp: TcpStream,
    dribble: Option<Duration>,
}

impl ServerTls<'_, '_> {
    fn flush_records(&mut self) -> io::Result<()> {
        while !self.conn.send_buf().is_empty() {
            let k = match self.dribble.filter(|_| !self.conn.is_handshaking()) {
                Some(delay) => {
                    std::thread::sleep(delay);
                    self.tcp.write(&self.conn.send_buf()[..1])?
                }
                None => self.tcp.write(self.conn.send_buf())?,
            };
            self.conn.send_ack(k).map_err(|e| io::Error::other(format!("{e:?}")))?;
        }
        Ok(())
    }
}

impl Read for ServerTls<'_, '_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            let plain = self.conn.read_plain();
            if !plain.is_empty() {
                let k = plain.len().min(out.len());
                out[..k].copy_from_slice(&plain[..k]);
                self.conn.read_ack(k).map_err(|e| io::Error::other(format!("{e:?}")))?;
                return Ok(k);
            }
            if self.conn.peer_closed() {
                return Ok(0);
            }
            self.flush_records()?;
            let k = self.tcp.read(self.conn.recv_buf())?;
            if k == 0 {
                return Ok(0);
            }
            let acked = self.conn.recv_ack(k);
            self.flush_records()?;
            acked.map_err(|e| io::Error::other(format!("{e:?}")))?;
        }
    }
}

impl Write for ServerTls<'_, '_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let room = self.conn.write_plain().map_err(|e| io::Error::other(format!("{e:?}")))?;
        let k = room.len().min(data.len());
        room[..k].copy_from_slice(&data[..k]);
        self.conn.encrypt_in_place(k).map_err(|e| io::Error::other(format!("{e:?}")))?;
        self.flush_records()?;
        Ok(k)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_records()
    }
}

fn serve_tls_conn(tcp: TcpStream, n: usize, h: &Handler, seen: &Mutex<Vec<Req>>, dribble: Option<Duration>) -> io::Result<()> {
    let key = SigningKey::from_pkcs8_der(KEY).unwrap();
    let chain: [&[u8]; 1] = [LEAF];
    let config = ServerConfig::new(&chain, &key).unwrap();
    let mut recv = vec![0u8; RECOMMENDED_BUFFER_LEN];
    let mut send = vec![0u8; RECOMMENDED_BUFFER_LEN];
    let conn = Connection::new(config, &mut madcawl_net::tls::OsEntropy, CpuFeatures::compile_time(), &mut recv, &mut send)
        .map_err(|e| io::Error::other(format!("{e:?}")))?;
    let mut s = ServerTls { conn, tcp, dribble };
    while s.conn.is_handshaking() {
        s.flush_records()?;
        let k = s.tcp.read(s.conn.recv_buf())?;
        if k == 0 {
            return Ok(());
        }
        let acked = s.conn.recv_ack(k);
        s.flush_records()?;
        acked.map_err(|e| io::Error::other(format!("{e:?}")))?;
    }
    serve_conn(&mut s, n, h, seen)
}
