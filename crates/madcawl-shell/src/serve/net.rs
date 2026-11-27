//! The host's network: the network service (`madcawl-netd`'s guarded
//! service, run on a thread of this process, or with `--isolate` in a
//! process of its own, see `netproc`), the control channel the
//! host registers on, and a connection per page. A page in this host is
//! the host's own: its navigations and the requests of its document go
//! on a connection registered for the host, which the service lets
//! through as sent. Each agent session's pages are in a cookie partition
//! of the session's own, opened with its first web page and dropped when
//! the session ends; partitions live in memory only and the service has
//! no profile, so an agent's pages never carry the user's logins, nor
//! another session's.

use std::io;
use std::os::unix::fs::FileTypeExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar};
use std::time::{Duration, Instant};

use madcawl_net::wire::{Frame, kind, tag};
use madcawl_net::{Client, Config};
use madcawl_netd::{Downloads, ServerLimits, serve_guarded};

use super::link::Link;
pub use super::netctl::NetConn;
use super::netctl::{Control, ControlLock};
use super::netproc::{self, Process, Spec};

/// How long a control-channel answer or a write to the service may take.
const CONTROL_WAIT: Duration = Duration::from_secs(10);

pub struct Net {
    socket: PathBuf,
    control: Channel,
    /// Signalled when the network process is replaced, or will not be
    replaced: Condvar,
    pub downloads: Option<PathBuf>,
    /// The network process, when the service runs in one of its own
    pub process: Option<Process>,
    /// Registrations content processes hold through the host, counted
    /// against the service's limit
    slots: AtomicUsize,
}

/// The control channel is the host's own; a content process has none and
/// asks its host to register and release its pages' connections, which
/// the host does only in the session's own partition.
enum Channel {
    Own(ControlLock),
    Host(Arc<Link>),
}

/// How long a request that found the network process gone waits for the
/// next one.
const RESTART_WAIT: Duration = Duration::from_secs(10);
/// A session's cookie partition; never 0, which is the profile's.
pub(crate) fn partition_of(session: u32) -> u64 {
    u64::from(session) + 1
}

impl Net {
    /// Starts the service on a socket at `socket`, saving downloads into
    /// `downloads` (each file at most `per_file` bytes, the directory at
    /// most `total`), with at most `sessions` cookie partitions open.
    pub fn start(socket: &Path, downloads: Option<(&Path, u64, u64)>, sessions: usize) -> io::Result<Net> {
        if std::fs::symlink_metadata(socket).is_ok_and(|m| m.file_type().is_socket()) {
            std::fs::remove_file(socket)?;
        }
        let listener = UnixListener::bind(socket)?;
        let (host, theirs) = UnixStream::pair()?;
        let control = Control::new(host, CONTROL_WAIT)?;
        let d = match downloads {
            Some((dir, per_file, total)) => {
                std::fs::create_dir_all(dir)?;
                Some(Arc::new(Downloads::with_caps(dir.to_path_buf(), per_file, total, 10_000)))
            }
            None => None,
        };
        let client = Arc::new(Client::new(Config { partitions: sessions, ..Config::default() }));
        std::thread::Builder::new().name("netd".into()).spawn(move || {
            if let Err(e) = serve_guarded(listener, theirs, client, ServerLimits::default(), d) {
                eprintln!("madcawl-shell: network service: {e}");
            }
        })?;
        Ok(Net::new(socket, control, downloads.map(|d| d.0.to_path_buf()), None))
    }

    pub(super) fn new(socket: &Path, control: Control, downloads: Option<PathBuf>, process: Option<Process>) -> Net {
        let control = Channel::Own(ControlLock::new(control));
        Net { socket: socket.to_path_buf(), control, replaced: Condvar::new(), downloads, process, slots: AtomicUsize::new(0) }
    }

    /// The network of a content process, whose host holds the control
    /// channel and answers its frames through `link`.
    pub fn through(link: Arc<Link>, socket: &Path, downloads: Option<PathBuf>) -> Net {
        let control = Channel::Host(link);
        Net { socket: socket.to_path_buf(), control, replaced: Condvar::new(), downloads, process: None, slots: AtomicUsize::new(0) }
    }

    fn own(&self) -> &ControlLock {
        match &self.control {
            Channel::Own(c) => c,
            Channel::Host(_) => unreachable!("only a host supervises its network process"),
        }
    }

    /// Starts the service in a process of its own, `exe --netd`, and keeps
    /// it running (see `netproc`); otherwise as `start`.
    pub fn spawn(exe: &Path, socket: &Path, downloads: Option<(&Path, u64, u64)>, sessions: usize) -> io::Result<Arc<Net>> {
        let spec = Spec::new(exe, socket, sessions, downloads);
        let dir = downloads.map(|d| d.0.to_path_buf());
        netproc::start(spec, |host, pid| {
            let process = Process::default();
            process.pid.store(pid, Ordering::SeqCst);
            Ok(Arc::new(Net::new(socket, Control::new(host, CONTROL_WAIT)?, dir, Some(process))))
        })
    }

    /// The network process was replaced by process `pid`, whose control
    /// channel `host` is.
    pub(crate) fn replaced(&self, host: UnixStream, pid: i32) {
        let mut c = self.own().lock();
        match Control::new(host, CONTROL_WAIT) {
            Ok(mut new) => {
                new.generation = c.generation + 1;
                *c = new;
            }
            Err(e) => eprintln!("madcawl-shell: the new network process's channel: {e}"),
        }
        if let Some(p) = &self.process {
            p.pid.store(pid, Ordering::SeqCst);
            p.restarts.fetch_add(1, Ordering::SeqCst);
        }
        // the new process holds no registrations
        self.slots.store(0, Ordering::SeqCst);
        drop(c);
        self.replaced.notify_all();
    }

    /// The network process will not be started again.
    pub(crate) fn braked(&self) {
        self.own().lock().braked = true;
        self.replaced.notify_all();
    }

    /// One request on the control channel. When the network process is
    /// found gone, the request waits for the next one and goes to it.
    /// Each wait is cut short at `by` when there is one.
    fn control(&self, f: Frame, by: Option<Instant>) -> io::Result<Frame> {
        let wait = |d: Duration| by.map_or(d, |b| d.min(b.saturating_duration_since(Instant::now())));
        let mut c = self.own().lock_by(by, &f)?;
        let generation = c.generation;
        match c.request(f.clone(), wait(CONTROL_WAIT)) {
            Err(e) if self.process.is_some() && e.kind() != io::ErrorKind::TimedOut => {
                let replaced = self.replaced.wait_timeout_while(c, wait(RESTART_WAIT), |c| c.generation == generation && !c.braked);
                let mut c = replaced.unwrap_or_else(|e| e.into_inner()).0;
                if c.generation == generation {
                    return Err(e);
                }
                c.request(f, wait(CONTROL_WAIT))
            }
            r => r,
        }
    }

    /// Which network process the control channel is to, counting from 0;
    /// a new one starts with every session's cookie jar empty.
    pub fn generation(&self) -> u64 {
        self.process.as_ref().map_or(0, |p| u64::from(p.restarts.load(Ordering::SeqCst)))
    }

    /// How many registrations the host may hold for content processes:
    /// the limit the service itself is started with, which its
    /// connections share, so that a session the host refuses no more is
    /// not left to the service to refuse.
    pub fn slot_limit(&self) -> usize {
        ServerLimits::default().clients
    }

    /// Counts one more registration against `slot_limit`; whether there
    /// was room.
    pub fn take_slot(&self) -> bool {
        let limit = self.slot_limit();
        self.slots.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| (n < limit).then_some(n + 1)).is_ok()
    }

    /// Gives back `n` registrations made in network process `generation`;
    /// those of a process since replaced went with it.
    pub fn give_slots(&self, n: usize, generation: u64) {
        if n > 0 && generation == self.generation() {
            let _ = self.slots.try_update(Ordering::SeqCst, Ordering::SeqCst, |c| Some(c.saturating_sub(n)));
        }
    }

    /// Registers a connection of the host's own for agent session
    /// `session`, in that session's cookie partition; its token, and the
    /// generation of the network process it was made in.
    pub fn register(&self, session: u32) -> io::Result<(Vec<u8>, u64)> {
        self.register_by(session, None)
    }

    pub(crate) fn register_by(&self, session: u32, by: Option<Instant>) -> io::Result<(Vec<u8>, u64)> {
        let reg = Frame::new(kind::REGISTER).num(tag::NAVIGATION, 1).num(tag::PARTITION, partition_of(session));
        let r = self.control(reg, by)?;
        match (r.kind, r.field(tag::TOKEN)) {
            (kind::REGISTERED, Some(t)) => Ok((t.to_vec(), self.generation())),
            _ => Err(io::Error::other(r.get_str(tag::MESSAGE).unwrap_or("not registered").to_owned())),
        }
    }

    /// Drops the registration `token`; whether the service knew it.
    pub fn unregister(&self, token: &[u8]) -> io::Result<bool> {
        self.unregister_by(token, None)
    }

    pub(crate) fn unregister_by(&self, token: &[u8], by: Option<Instant>) -> io::Result<bool> {
        Ok(self.control(Frame::new(kind::UNREGISTER).bytes(tag::TOKEN, token), by)?.kind == kind::REGISTERED)
    }

    /// A connection of the host's own, for one page of agent session
    /// `session`, in that session's cookie partition.
    pub fn connect(&self, session: u32) -> io::Result<NetConn> {
        self.connect_by(session, None)
    }

    /// As `connect`, given up at `by` when there is one; a content
    /// process's host is given `by` with the question and gives up then.
    pub fn connect_by(&self, session: u32, by: Option<Instant>) -> io::Result<NetConn> {
        let token = match &self.control {
            Channel::Own(_) => self.register_by(session, by)?.0,
            Channel::Host(link) => link.net_register(by)?,
        };
        let hello = by.unwrap_or_else(|| Instant::now() + CONTROL_WAIT);
        NetConn::open(&self.socket, token.clone(), CONTROL_WAIT, hello).inspect_err(|_| self.release_token(&token, by))
    }

    /// Sends a FETCH for a page of session `session` on `conn`, connected
    /// first when it is not; its id. A connection to a network process
    /// that has since been replaced gets one more try on a new one.
    pub fn fetch(&self, conn: &mut Option<NetConn>, session: u32, f: Frame) -> io::Result<u64> {
        if let Some(c) = conn.as_mut()
            && let Ok(id) = c.fetch(f.clone())
        {
            return Ok(id);
        }
        conn.insert(self.connect(session)?).fetch(f)
    }

    /// Drops a page's registration; its connection is refused after.
    pub fn release(&self, conn: &NetConn) {
        self.release_token(&conn.token, None);
    }

    /// As `release`, given up at `by` as `connect_by` is.
    pub fn release_by(&self, conn: &NetConn, by: Option<Instant>) {
        self.release_token(&conn.token, by);
    }

    fn release_token(&self, token: &[u8], by: Option<Instant>) {
        let _ = match &self.control {
            Channel::Own(_) => self.unregister_by(token, by).map(drop),
            Channel::Host(link) => link.net_release(token, by),
        };
    }

    /// Drops agent session `session`'s cookie partition and its cookies;
    /// whether it was open.
    pub fn end_session(&self, session: u32) -> io::Result<bool> {
        // the host drops it once the content process has ended
        if matches!(self.control, Channel::Host(_)) {
            return Ok(true);
        }
        let r = self.control(Frame::new(kind::PARTITION_DROP).num(tag::PARTITION, partition_of(session)), None)?;
        match r.kind {
            kind::PARTITION_DROP => Ok(r.get_num(tag::OK) == Some(1)),
            _ => Err(io::Error::other(r.get_str(tag::MESSAGE).unwrap_or("not dropped").to_owned())),
        }
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        if let Some(p) = &self.process {
            p.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_session_is_in_the_profiles_partition() {
        assert_eq!(partition_of(0), 1);
        assert_eq!(partition_of(u32::MAX), u64::from(u32::MAX) + 1);
    }
}
