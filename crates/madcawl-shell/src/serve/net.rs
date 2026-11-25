//! The host's network: the network service (`madcawl-netd`'s guarded
//! service, run on a thread of this process), the control channel the
//! host registers on, and a connection per page. A page in this host is
//! the host's own: its navigations and the requests of its document go
//! on a connection registered for the host, which the service lets
//! through as sent. Its cookie jar starts empty and is never saved, so
//! an agent's pages never carry the user's logins.

use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::FileTypeExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use madcawl_net::wire::{Frame, FrameError, MAX_FRAME, kind, tag};
use madcawl_net::{Client, Config};
use madcawl_netd::{Downloads, ServerLimits, serve_guarded};

/// How long a control-channel answer or a write to the service may take.
const CONTROL_WAIT: Duration = Duration::from_secs(10);

pub struct Net {
    socket: PathBuf,
    control: Mutex<UnixStream>,
    pub downloads: Option<PathBuf>,
}

impl Net {
    /// Starts the service on a socket at `socket`, saving downloads into
    /// `downloads` (each file at most `per_file` bytes, the directory at
    /// most `total`).
    pub fn start(socket: &Path, downloads: Option<(&Path, u64, u64)>) -> io::Result<Net> {
        if std::fs::symlink_metadata(socket).is_ok_and(|m| m.file_type().is_socket()) {
            std::fs::remove_file(socket)?;
        }
        let listener = UnixListener::bind(socket)?;
        let (host, theirs) = UnixStream::pair()?;
        host.set_read_timeout(Some(CONTROL_WAIT))?;
        host.set_write_timeout(Some(CONTROL_WAIT))?;
        let d = match downloads {
            Some((dir, per_file, total)) => {
                std::fs::create_dir_all(dir)?;
                Some(Arc::new(Downloads::with_caps(dir.to_path_buf(), per_file, total, 10_000)))
            }
            None => None,
        };
        let client = Arc::new(Client::new(Config::default()));
        std::thread::Builder::new().name("netd".into()).spawn(move || {
            if let Err(e) = serve_guarded(listener, theirs, client, ServerLimits::default(), d) {
                eprintln!("madcawl-shell: network service: {e}");
            }
        })?;
        Ok(Net { socket: socket.to_path_buf(), control: Mutex::new(host), downloads: downloads.map(|d| d.0.to_path_buf()) })
    }

    fn control(&self, f: &Frame) -> io::Result<Frame> {
        let mut c = self.control.lock().unwrap_or_else(|e| e.into_inner());
        f.write_to(&mut *c)?;
        Frame::read_from(&mut *c)?.ok_or_else(|| io::Error::other("the network service closed its control channel"))
    }

    /// A connection of the host's own, for one page.
    pub fn connect(&self) -> io::Result<NetConn> {
        let r = self.control(&Frame::new(kind::REGISTER).num(tag::ID, 1).num(tag::NAVIGATION, 1))?;
        let token = match (r.kind, r.field(tag::TOKEN)) {
            (kind::REGISTERED, Some(t)) => t.to_vec(),
            _ => return Err(io::Error::other(r.get_str(tag::MESSAGE).unwrap_or("not registered").to_owned())),
        };
        let mut s = UnixStream::connect(&self.socket)?;
        s.set_write_timeout(Some(CONTROL_WAIT))?;
        let mut conn = NetConn { s: s.try_clone()?, buf: Vec::new(), token, next_id: 1 };
        Frame::new(kind::HELLO).num(tag::ID, 0).bytes(tag::TOKEN, &conn.token).write_to(&mut s)?;
        match conn.next(Instant::now() + CONTROL_WAIT)? {
            Some(f) if f.kind == kind::HELLO && f.get_num(tag::OK) == Some(1) => Ok(conn),
            _ => Err(io::Error::other("the network service refused the host's connection")),
        }
    }

    /// Drops a page's registration; its connection is refused after.
    pub fn release(&self, conn: &NetConn) {
        let _ = self.control(&Frame::new(kind::UNREGISTER).num(tag::ID, 1).bytes(tag::TOKEN, &conn.token));
    }
}

/// One page's connection to the network service: frames out, and frames
/// in as they arrive, kept whole across reads that time out.
pub struct NetConn {
    s: UnixStream,
    buf: Vec<u8>,
    token: Vec<u8>,
    next_id: u64,
}

impl NetConn {
    /// Sends a FETCH; its id.
    pub fn fetch(&mut self, f: Frame) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        let mut f = f;
        f.fields.retain(|(t, _)| *t != tag::ID);
        f.fields.insert(0, (tag::ID, varint(id)));
        let mut out = Vec::new();
        f.encode(&mut out);
        self.s.write_all(&out)?;
        Ok(id)
    }

    /// The next frame, or `None` once `deadline` passes with no whole
    /// frame in.
    pub fn next(&mut self, deadline: Instant) -> io::Result<Option<Frame>> {
        loop {
            match Frame::decode(&self.buf) {
                Ok((f, used)) => {
                    self.buf.drain(..used);
                    return Ok(Some(f));
                }
                Err(FrameError::Short) => {}
                Err(e) => return Err(io::Error::other(format!("a bad frame from the network service: {e:?}"))),
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.s.set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
            let mut chunk = [0u8; 64 << 10];
            match self.s.read(&mut chunk) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) if self.buf.len() + n > MAX_FRAME + 4 => {
                    return Err(io::Error::other("a frame longer than the network service sends"));
                }
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                    return Ok(None);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }
}

fn varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
    out
}
