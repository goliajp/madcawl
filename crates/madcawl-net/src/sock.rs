//! The TCP socket under a connection. Every read and write on it blocks
//! for at most the per-operation timeout and never past the request's
//! deadline, and a cancelled request's sockets are shut down so a blocked
//! read or write returns at once.

use crate::error::NetError;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// A cancellation shared by any number of requests; once cancelled it
/// stays so, and the sockets those requests are using are shut down.
#[derive(Debug, Default)]
pub struct Cancel {
    flag: AtomicBool,
    next: AtomicU64,
    /// duplicates of the sockets in use, by attachment
    socks: Mutex<Vec<(u64, TcpStream)>>,
}

impl Cancel {
    pub fn new() -> Arc<Cancel> {
        Arc::new(Cancel::default())
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        // under the lock: a request that detached has gone through it, so
        // a socket it put back in the pool for another request is never
        // shut down here
        let mut socks = self.socks.lock().unwrap_or_else(PoisonError::into_inner);
        for (_, s) in socks.drain(..) {
            let _ = s.shutdown(Shutdown::Both);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Registers `tcp` until the returned guard drops, so a cancellation
    /// in the meantime shuts it down.
    pub fn attach(&self, tcp: &TcpStream) -> Result<Attached<'_>, NetError> {
        let dup = tcp.try_clone()?;
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut socks = self.socks.lock().unwrap_or_else(PoisonError::into_inner);
        // checked under the lock: cancel() sets the flag before taking it
        if self.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        socks.push((id, dup));
        Ok(Attached { cancel: self, id })
    }
}

pub struct Attached<'a> {
    cancel: &'a Cancel,
    id: u64,
}

impl Drop for Attached<'_> {
    fn drop(&mut self) {
        self.cancel.socks.lock().unwrap_or_else(PoisonError::into_inner).retain(|(i, _)| *i != self.id);
    }
}

/// What bounds the socket operations of the request using a connection.
#[derive(Clone, Debug)]
pub struct Bounds {
    /// how long one read or write may block
    pub io: Duration,
    pub deadline: Option<Instant>,
    pub cancel: Option<Arc<Cancel>>,
}

impl Bounds {
    pub fn idle(io: Duration) -> Self {
        Bounds { io, deadline: None, cancel: None }
    }
}

pub struct Tcp {
    sock: TcpStream,
    bounds: Bounds,
    /// the timeouts last set on the socket, read then write
    set: [Option<Duration>; 2],
}

fn other(e: NetError) -> io::Error {
    io::Error::other(e)
}

impl Tcp {
    pub fn new(sock: TcpStream, bounds: Bounds) -> Self {
        Tcp { sock, bounds, set: [None; 2] }
    }

    pub fn sock(&self) -> &TcpStream {
        &self.sock
    }

    pub fn set_bounds(&mut self, bounds: Bounds) {
        self.bounds = bounds;
    }

    pub fn bounds(&self) -> &Bounds {
        &self.bounds
    }

    fn cancelled(&self) -> bool {
        self.bounds.cancel.as_ref().is_some_and(|c| c.is_cancelled())
    }

    /// Sets the socket's timeout for the next read (0) or write (1) to
    /// what is left of the operation's and the request's time.
    fn arm(&mut self, which: usize) -> io::Result<()> {
        if self.cancelled() {
            return Err(other(NetError::Cancelled));
        }
        let mut t = self.bounds.io;
        if let Some(d) = self.bounds.deadline {
            let left = d.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(other(NetError::Deadline));
            }
            t = t.min(left);
        }
        if self.set[which] != Some(t) {
            match which {
                0 => self.sock.set_read_timeout(Some(t))?,
                _ => self.sock.set_write_timeout(Some(t))?,
            }
            self.set[which] = Some(t);
        }
        Ok(())
    }

    /// Names a failure for what caused it: a cancellation, the deadline,
    /// or the socket's own error.
    fn settle(&self, e: io::Error) -> io::Error {
        if self.cancelled() {
            other(NetError::Cancelled)
        } else if self.bounds.deadline.is_some_and(|d| Instant::now() >= d) {
            other(NetError::Deadline)
        } else {
            e
        }
    }
}

impl Read for Tcp {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.arm(0)?;
        match self.sock.read(buf) {
            // a cancellation shuts the socket down, which reads as an end
            Ok(0) if !buf.is_empty() && self.cancelled() => Err(other(NetError::Cancelled)),
            Ok(n) => Ok(n),
            Err(e) => Err(self.settle(e)),
        }
    }
}

impl Write for Tcp {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.arm(1)?;
        self.sock.write(buf).map_err(|e| self.settle(e))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
