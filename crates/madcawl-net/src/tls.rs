//! A TLS 1.3 client stream: `torajs-tls`'s sans-I/O connection driven over
//! a blocking `TcpStream`.

use crate::error::NetError;
use crate::trust::{Trust, take_rejection_reason};
use std::io::{self, Read, Write};
use std::mem::ManuallyDrop;
use std::net::{IpAddr, TcpStream};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use torajs_tls::client::{ClientConfig, ClientHello, Connection, DnsName, RECOMMENDED_BUFFER_LEN, ServerCheck};
use torajs_tls::torajs_crypto::cpu::CpuFeatures;
use torajs_tls::torajs_crypto::rand::{EntropyError, EntropySource};
use torajs_tls::torajs_x509::{ServerName, UnixTime};
use torajs_tls::{Error, MIN_BUFFER_LEN};

const ALPN: &[&[u8]] = &[b"http/1.1"];

unsafe extern "C" {
    fn arc4random_buf(buf: *mut u8, n: usize);
}

/// The kernel's random number generator.
pub struct OsEntropy;

impl EntropySource for OsEntropy {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), EntropyError> {
        // SAFETY: writes exactly `buf.len()` bytes into `buf`
        unsafe { arc4random_buf(buf.as_mut_ptr(), buf.len()) };
        Ok(())
    }
}

type Conn = Connection<'static, 'static, &'static Trust>;

/// The connection borrows its buffers, host name and verifier, so they
/// live on the heap behind raw pointers this value frees after the
/// connection is dropped.
pub struct TlsStream {
    conn: ManuallyDrop<Conn>,
    tcp: TcpStream,
    recv: *mut [u8],
    send: *mut [u8],
    host: *mut str,
    trust: Arc<Trust>,
}

// SAFETY: the raw pointers are owned heap blocks used only through `conn`,
// which holds plain data; nothing is shared with another thread
unsafe impl Send for TlsStream {}

impl Drop for TlsStream {
    fn drop(&mut self) {
        // SAFETY: the connection goes first, then what it borrowed; each
        // pointer came from Box::into_raw and is freed once
        unsafe {
            ManuallyDrop::drop(&mut self.conn);
            drop(Box::from_raw(self.recv));
            drop(Box::from_raw(self.send));
            drop(Box::from_raw(self.host));
        }
    }
}

pub fn tls_error(e: Error) -> NetError {
    match e {
        Error::Certificate(c) => NetError::Untrusted(format!("{c:?}")),
        Error::CertificateRejected(a) => NetError::Untrusted(match take_rejection_reason() {
            Some(reason) => format!("{reason} ({a:?})"),
            None => format!("{a:?}"),
        }),
        e => NetError::Tls(format!("{e:?}")),
    }
}

fn io_tls(e: Error) -> io::Error {
    io::Error::other(tls_error(e))
}

fn now() -> UnixTime {
    UnixTime::from_secs(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()))
}

impl TlsStream {
    /// Runs the handshake for `host` over `tcp`, which carries the timeouts
    /// the caller set.
    pub fn handshake(tcp: TcpStream, host: &str, trust: Arc<Trust>) -> Result<Self, NetError> {
        take_rejection_reason();
        let host: *mut str = Box::into_raw(host.to_owned().into_boxed_str());
        let recv: *mut [u8] = Box::into_raw(vec![0u8; RECOMMENDED_BUFFER_LEN].into_boxed_slice());
        let send: *mut [u8] = Box::into_raw(vec![0u8; MIN_BUFFER_LEN].into_boxed_slice());
        // SAFETY: the three blocks and the Arc'd verifier outlive `conn`
        // (see Drop), and nothing else touches them
        let (h, r, s, t): (&'static str, &'static mut [u8], &'static mut [u8], &'static Trust) =
            unsafe { (&*host, &mut *recv, &mut *send, &*Arc::as_ptr(&trust)) };
        let started = start(h, r, s, t);
        let conn = match started {
            Ok(c) => c,
            Err(e) => {
                // SAFETY: `start` failed and dropped its borrows
                unsafe {
                    drop(Box::from_raw(recv));
                    drop(Box::from_raw(send));
                    drop(Box::from_raw(host));
                }
                return Err(e);
            }
        };
        let mut stream = TlsStream { conn: ManuallyDrop::new(conn), tcp, recv, send, host, trust };
        stream.drive_handshake()?;
        Ok(stream)
    }

    fn drive_handshake(&mut self) -> Result<(), NetError> {
        loop {
            self.flush_records()?;
            if !self.conn.is_handshaking() {
                return Ok(());
            }
            let room = self.conn.recv_buf();
            if room.is_empty() {
                return Err(NetError::Tls("receive buffer full during the handshake".into()));
            }
            let n = self.tcp.read(room)?;
            if n == 0 {
                return Err(NetError::Tls("connection closed during the handshake".into()));
            }
            if let Err(e) = self.conn.recv_ack(n) {
                let _ = self.flush_records();
                return Err(tls_error(e));
            }
        }
    }

    fn flush_records(&mut self) -> io::Result<()> {
        while !self.conn.send_buf().is_empty() {
            let n = self.tcp.write(self.conn.send_buf())?;
            if n == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            self.conn.send_ack(n).map_err(io_tls)?;
        }
        Ok(())
    }

    pub fn tcp(&self) -> &TcpStream {
        &self.tcp
    }

    pub fn alpn(&self) -> Option<&[u8]> {
        self.conn.alpn()
    }

    pub fn trust(&self) -> &Arc<Trust> {
        &self.trust
    }

    /// Sends `close_notify`, best effort.
    pub fn close(&mut self) {
        if self.conn.close().is_ok() {
            let _ = self.flush_records();
        }
    }
}

fn start(host: &'static str, recv: &'static mut [u8], send: &'static mut [u8], trust: &'static Trust) -> Result<Conn, NetError> {
    let ip = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    let mut config = ClientConfig::new().with_alpn(ALPN).map_err(tls_error)?;
    let mut check = ServerCheck::new(trust, now());
    if ip.parse::<IpAddr>().is_ok() {
        check = check.with_server_name(ServerName::try_from(ip).map_err(|e| NetError::Tls(format!("{e:?}")))?);
    } else {
        config = config.with_server_name(DnsName::new(host).map_err(tls_error)?);
    }
    let hello = ClientHello::new(config, &mut OsEntropy).map_err(tls_error)?;
    Connection::new(hello, check, CpuFeatures::compile_time(), recv, send).map_err(tls_error)
}

impl Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let plain = self.conn.read_plain();
            if !plain.is_empty() {
                let n = plain.len().min(buf.len());
                buf[..n].copy_from_slice(&plain[..n]);
                self.conn.read_ack(n).map_err(io_tls)?;
                return Ok(n);
            }
            if self.conn.peer_closed() {
                return Ok(0);
            }
            if let Some(e) = self.conn.error() {
                return Err(io_tls(e));
            }
            self.flush_records()?;
            let room = self.conn.recv_buf();
            if room.is_empty() {
                return Err(io::Error::other("tls receive buffer stuck"));
            }
            let n = self.tcp.read(room)?;
            if n == 0 {
                return Ok(0);
            }
            if let Err(e) = self.conn.recv_ack(n) {
                let _ = self.flush_records();
                return Err(io_tls(e));
            }
        }
    }
}

impl Write for TlsStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        for _ in 0..2 {
            let room = self.conn.write_plain().map_err(io_tls)?;
            if !room.is_empty() {
                let n = room.len().min(buf.len());
                room[..n].copy_from_slice(&buf[..n]);
                self.conn.encrypt_in_place(n).map_err(io_tls)?;
                self.flush_records()?;
                return Ok(n);
            }
            self.flush_records()?;
        }
        Err(io::Error::other("tls send buffer has no room"))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_records()
    }
}
