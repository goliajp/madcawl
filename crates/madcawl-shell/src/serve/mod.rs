//! `madcawl-shell --serve <socket>`: the host outside marspot. It
//! listens on a Unix socket and plays the proxy's trusted part for
//! every agent session that connects (one connection is one session):
//! it opens windowless pages from local files, and with `--net` from
//! the network through its own network service; it answers the agent
//! protocol's view, diff, action, wait, request-log and screenshot
//! requests on them, only on the session's own pages; it puts every
//! reply that carries page content in a fresh envelope, and writes a
//! line to the audit log for every request. Sessions, pages per session, frame sizes, each
//! page's change ring and request log are bounded; a page nobody asked
//! anything of for the idle time is closed, and a session's pages close
//! with it.

mod act;
mod act_input;
mod activation;
mod config;
mod conn;
mod doc;
mod limit;
mod loader;
mod log;
pub mod memory;
mod net;
mod netctl;
pub mod netproc;
mod orphan;
mod page;
mod reqlog;
mod shot;
mod socket;
mod submit;
mod target;
mod wait;
pub mod watch;
mod wire;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use madcawl_agent::{Frame, kind, tag};

pub use config::{Config, USAGE, parse_args};
pub use log::AuditLog;
pub use net::Net;
pub use reqlog::{ERROR_CAP, LOG_CAP};
pub use submit::origin;
pub use target::{TargetError, resolve, scheme};

/// The error codes the host adds to the agent protocol's own (1 to 5).
pub mod code {
    pub const UNKNOWN: u64 = 6;
    pub const BAD_REQUEST: u64 = 10;
    /// Not a local file: another scheme, or a host
    pub const REFUSED: u64 = 11;
    pub const NOT_ALLOWED: u64 = 12;
    pub const TOO_MANY_PAGES: u64 = 13;
    pub const NO_PAGE: u64 = 14;
    pub const IO: u64 = 15;
    pub const REPLY_TOO_LARGE: u64 = 16;
    pub const REQUEST_TOO_LARGE: u64 = 17;
    pub const TOO_MANY_SESSIONS: u64 = 18;
    pub const RATE_LIMITED: u64 = 19;
    /// A check of an action's target failed, or it cannot take the action
    pub const CHECK: u64 = 20;
    /// The page is another session's
    pub const DENIED: u64 = 21;
    /// Needs the user's approval, which the host was not started with
    pub const APPROVAL: u64 = 22;
    /// A navigation could not start
    pub const NAVIGATION: u64 = 23;
}

pub(crate) struct Shared {
    pub config: Config,
    pub log: AuditLog,
    pub next_session: AtomicU32,
    pub active: AtomicUsize,
    /// Pages open across every session
    pub pages: AtomicUsize,
    /// When the last session ended, or the host started
    pub quiet_since: Mutex<Instant>,
    /// Page numbers are the host's, so one session cannot name another's
    /// page by accident; each page's session
    pub next_page: AtomicU32,
    pub owners: Mutex<HashMap<u32, u32>>,
    pub net: Option<Arc<Net>>,
    /// The host's start time and process, as session directories are
    /// named by
    pub run: String,
}

pub struct Server {
    listener: std::os::unix::net::UnixListener,
    shared: Arc<Shared>,
}

/// The UTC start time to the second and the process id, e.g.
/// `20261125T093000-4242`.
fn run_tag() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, s) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = log::civil(days);
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}-{}", s / 3600, s / 60 % 60, s % 60, std::process::id())
}

pub(crate) fn error_frame(code: u64, text: &str) -> Frame {
    Frame::new(kind::ERROR)
        .num(tag::CODE, code)
        .str(tag::TEXT, text)
}

/// Writes `f` with a short deadline: a client that does not read must
/// not hold the accepting thread.
fn refuse(stream: &mut std::os::unix::net::UnixStream, f: &Frame) {
    let mut out = Vec::new();
    f.encode(&mut out);
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let _ = std::io::Write::write_all(stream, &out);
}

impl Server {
    /// Binds the socket (only the user may connect, see `socket::bind`),
    /// or takes the inherited one `listen_fd` names, and opens the audit
    /// log.
    pub fn bind(config: Config) -> std::io::Result<Server> {
        let listener = match config.listen_fd {
            Some(fd) => activation::inherited(fd)?,
            None => socket::bind(&config.socket)?,
        };
        let log = AuditLog::open(&config.audit)?;
        let net = match config.net {
            true => {
                let mut sock = config.socket.clone().into_os_string();
                sock.push(".net");
                let d = config.downloads.as_deref().map(|d| (d, config.download_cap, config.download_cap * 4));
                let sock = std::path::Path::new(&sock);
                match &config.isolate {
                    Some(exe) => Some(Net::spawn(exe, sock, d, config.max_sessions)?),
                    None => Some(Arc::new(Net::start(sock, d, config.max_sessions)?)),
                }
            }
            false => None,
        };
        let shared = Arc::new(Shared {
            config,
            log,
            next_session: AtomicU32::new(1),
            active: AtomicUsize::new(0),
            pages: AtomicUsize::new(0),
            quiet_since: Mutex::new(Instant::now()),
            next_page: AtomicU32::new(1),
            owners: Mutex::new(HashMap::new()),
            net,
            run: run_tag(),
        });
        Ok(Server { listener, shared })
    }

    /// Accepts sessions, each on a thread of its own, for as long as the
    /// process lives, or with `exit_idle` until no session has been open
    /// for that long; a failed accept or spawn is reported and passed
    /// over.
    pub fn run(self) -> std::io::Result<()> {
        let Some(idle) = self.shared.config.exit_idle else {
            for stream in self.listener.incoming() {
                match stream {
                    // an inherited socket that cannot be accepted on
                    Err(e) if self.shared.config.listen_fd.is_some() && activation::fatal(&e) => return Err(e),
                    s => self.take(s),
                }
            }
            return Ok(());
        };
        let fd = std::os::fd::AsRawFd::as_raw_fd(&self.listener);
        loop {
            // while sessions are open, wake once an idle period to look
            // again: nothing is waited on in between
            let wait = if self.shared.active.load(Ordering::SeqCst) == 0 {
                let quiet = self.shared.quiet_since.lock().unwrap_or_else(|e| e.into_inner()).elapsed();
                match idle.checked_sub(quiet) {
                    Some(w) if !w.is_zero() => w,
                    _ => return Ok(()),
                }
            } else {
                idle
            };
            if activation::readable(fd, wait)? {
                match self.listener.accept() {
                    Err(e) if activation::fatal(&e) => return Err(e),
                    r => self.take(r.map(|(s, _)| s)),
                }
            }
        }
    }

    fn take(&self, stream: std::io::Result<std::os::unix::net::UnixStream>) {
        match stream {
            Ok(s) => self.start(s),
            Err(e) => {
                eprintln!("madcawl-shell: accept: {e}");
                // out of descriptors: give sessions a moment to end
                if matches!(e.raw_os_error(), Some(23 | 24)) {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }

    fn start(&self, mut stream: std::os::unix::net::UnixStream) {
        let shared = self.shared.clone();
        if shared.active.fetch_add(1, Ordering::SeqCst) >= shared.config.max_sessions {
            shared.active.fetch_sub(1, Ordering::SeqCst);
            let f = error_frame(
                code::TOO_MANY_SESSIONS,
                "too many agent sessions on this host",
            );
            refuse(&mut stream, &f);
            return;
        }
        let id = shared.next_session.fetch_add(1, Ordering::SeqCst);
        // the stream goes to the thread through a slot, so a failed spawn
        // leaves it here to answer on
        let slot = Arc::new(Mutex::new(Some(stream)));
        let theirs = slot.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("session-{id}"))
            .spawn(move || {
                let s = theirs.lock().unwrap_or_else(|e| e.into_inner()).take();
                if let Some(s) = s {
                    conn::Conn::new(id, &shared).run(s);
                }
                // the time first: a host counting no sessions must find it set
                *shared.quiet_since.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
                shared.active.fetch_sub(1, Ordering::SeqCst);
            });
        if let Err(e) = spawned {
            eprintln!("madcawl-shell: session thread: {e}");
            self.shared.active.fetch_sub(1, Ordering::SeqCst);
            let s = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(mut s) = s {
                refuse(
                    &mut s,
                    &error_frame(code::IO, "the host cannot start a session now"),
                );
            }
        }
    }
}
