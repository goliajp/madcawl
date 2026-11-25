//! A connection to an origin: plain TCP or TLS over it.

use crate::tls::TlsStream;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub enum Stream {
    Plain(TcpStream),
    Tls(Box<TlsStream>),
}

impl Stream {
    pub fn tcp(&self) -> &TcpStream {
        match self {
            Stream::Plain(s) => s,
            Stream::Tls(t) => t.tcp(),
        }
    }

    pub fn set_timeouts(&self, read: Duration, write: Duration) -> io::Result<()> {
        self.tcp().set_read_timeout(Some(read))?;
        self.tcp().set_write_timeout(Some(write))
    }

    /// Whether the peer closed or sent bytes while the connection sat idle;
    /// either way it cannot carry a new request.
    pub fn is_stale(&self) -> bool {
        let tcp = self.tcp();
        if tcp.set_nonblocking(true).is_err() {
            return true;
        }
        let mut b = [0u8; 1];
        let stale = !matches!(tcp.peek(&mut b), Err(ref e) if e.kind() == io::ErrorKind::WouldBlock);
        tcp.set_nonblocking(false).is_err() || stale
    }

    pub fn close(&mut self) {
        if let Stream::Tls(t) = self {
            t.close();
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(t) => t.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(t) => t.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(t) => t.flush(),
        }
    }
}
