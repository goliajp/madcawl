//! The network service in a process of its own (`--isolate`): the host
//! starts `madcawl-shell --netd`, which serves on the host's network
//! socket and takes its control channel on standard input, and restarts
//! it when it ends. Requests in flight when it ends fail; the pages
//! already loaded keep their documents; the next request a page makes
//! connects to the new process. Cookie partitions live in that process's
//! memory, so a restart empties every session's jar; each session that
//! had cookies is told so in its next reply. A process that
//! ends three times in a minute is not started again: the pages stay as
//! they are and loads from the network fail until the host restarts.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::os::fd::{AsFd as _, OwnedFd};
use std::os::unix::fs::FileTypeExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use madcawl_net::wire::{Frame, kind, tag};
use madcawl_net::{Client, Config};
use madcawl_netd::{Downloads, ServerLimits, serve_guarded};

use super::net::Net;

/// Crashes within `BRAKE_WINDOW` after which a process is not restarted.
pub const BRAKE_CRASHES: usize = 3;
pub const BRAKE_WINDOW: Duration = Duration::from_secs(60);

/// How long a new network process may take to answer its first frame.
const READY_WAIT: Duration = Duration::from_secs(10);

/// What the host knows of its network process.
#[derive(Default)]
pub struct Process {
    pub pid: AtomicI32,
    pub restarts: AtomicU32,
}

/// How to start the network process.
pub(crate) struct Spec {
    pub exe: PathBuf,
    pub args: Vec<OsString>,
}

impl Spec {
    pub fn new(exe: &Path, socket: &Path, sessions: usize, downloads: Option<(&Path, u64, u64)>) -> Spec {
        let mut args: Vec<OsString> = vec!["--netd".into(), socket.into(), "--partitions".into(), sessions.to_string().into()];
        if let Some((dir, per_file, total)) = downloads {
            args.extend(["--downloads".into(), dir.into(), per_file.to_string().into(), total.to_string().into()]);
        }
        Spec { exe: exe.to_path_buf(), args }
    }

    /// The process started, with the host's end of its control channel,
    /// once it has answered on it (its socket is bound by then).
    pub fn start(&self) -> io::Result<(Child, UnixStream)> {
        let (host, theirs) = UnixStream::pair()?;
        let mut child = Command::new(&self.exe)
            .args(&self.args)
            .stdin(Stdio::from(OwnedFd::from(theirs)))
            .stdout(Stdio::null())
            .spawn()?;
        if let Err(e) = ready(&host) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        Ok((child, host))
    }
}

/// Asks for the refusal count, a question any network process answers.
fn ready(host: &UnixStream) -> io::Result<()> {
    let mut s = host.try_clone()?;
    s.set_read_timeout(Some(READY_WAIT))?;
    Frame::new(kind::STATS).num(tag::ID, 0).write_to(&mut s)?;
    let r = Frame::read_from(&mut s).map_err(|e| io::Error::other(format!("{e:?}")))?;
    s.set_read_timeout(None)?;
    match r {
        Some(_) => Ok(()),
        None => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the network process closed its control channel")),
    }
}

/// Waits on the network process and starts another when it ends, until
/// the host is gone or the brake holds.
pub(crate) fn supervise(net: Weak<Net>, spec: Spec, mut child: Child) {
    let mut ended: VecDeque<Instant> = VecDeque::new();
    loop {
        let status = child.wait();
        let Some(net) = net.upgrade() else { return };
        let now = Instant::now();
        ended.push_back(now);
        while ended.front().is_some_and(|&t| now - t > BRAKE_WINDOW) {
            ended.pop_front();
        }
        let how = status.map_or_else(|e| e.to_string(), |s| s.to_string());
        if ended.len() >= BRAKE_CRASHES {
            eprintln!("madcawl-shell: the network process ended ({how}), {BRAKE_CRASHES} times within {BRAKE_WINDOW:?}: not started again");
            net.braked();
            return;
        }
        eprintln!("madcawl-shell: the network process ended ({how}); starting another");
        match spec.start() {
            Ok((c, host)) => {
                net.replaced(host, c.id() as i32);
                child = c;
            }
            Err(e) => {
                eprintln!("madcawl-shell: the network process could not start again: {e}");
                net.braked();
                return;
            }
        }
    }
}

/// Starts the service in a process and a thread to supervise it.
pub(crate) fn start(spec: Spec, net: impl FnOnce(UnixStream, i32) -> io::Result<Arc<Net>>) -> io::Result<Arc<Net>> {
    let (child, host) = spec.start()?;
    let net = net(host, child.id() as i32)?;
    let weak = Arc::downgrade(&net);
    std::thread::Builder::new().name("netd-supervisor".into()).spawn(move || supervise(weak, spec, child))?;
    Ok(net)
}

/// `madcawl-shell --netd <socket> --partitions N [--downloads <dir>
/// <per-file> <total>]`, with the control channel on standard input: the
/// network process. It exits with its host.
pub fn child_main(args: &[&str]) -> ExitCode {
    super::orphan::close_inherited_fds(None);
    let parsed = match args {
        [socket, "--partitions", n] => n.parse().ok().map(|n| (socket, n, None)),
        [socket, "--partitions", n, "--downloads", dir, per, total] => match (n.parse(), per.parse(), total.parse()) {
            (Ok(n), Ok(per), Ok(total)) => Some((socket, n, Some((PathBuf::from(dir), per, total)))),
            _ => None,
        },
        _ => None,
    };
    let Some((socket, partitions, downloads)) = parsed else {
        eprintln!("usage: madcawl-shell --netd <socket> --partitions N [--downloads <dir> <per-file> <total>]");
        return ExitCode::from(2);
    };
    super::orphan::exit_with_parent();
    match serve(Path::new(socket), partitions, downloads) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("madcawl-shell: network process: {e}");
            ExitCode::FAILURE
        }
    }
}

fn serve(socket: &Path, partitions: usize, downloads: Option<(PathBuf, u64, u64)>) -> io::Result<()> {
    let control = UnixStream::from(io::stdin().as_fd().try_clone_to_owned()?);
    let d = match downloads {
        Some((dir, per_file, total)) => {
            std::fs::create_dir_all(&dir)?;
            Some(Arc::new(Downloads::with_caps(dir, per_file, total, 10_000)))
        }
        None => None,
    };
    if std::fs::symlink_metadata(socket).is_ok_and(|m| m.file_type().is_socket()) {
        std::fs::remove_file(socket)?;
    }
    let listener = UnixListener::bind(socket)?;
    let client = Arc::new(Client::new(Config { partitions, ..Config::default() }));
    serve_guarded(listener, control, client, ServerLimits::default(), d)
}

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

impl Process {
    pub fn pid(&self) -> i32 {
        self.pid.load(Ordering::SeqCst)
    }

    /// Kills the network process, which this host started; its
    /// supervisor then finds the host's network gone and starts no other.
    pub fn kill(&self) {
        let pid = self.pid.swap(0, Ordering::SeqCst);
        if pid > 0 {
            // SAFETY: kill only sends a signal, to the process this host started
            unsafe { kill(pid, 9) };
        }
    }
}
