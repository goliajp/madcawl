//! `madcawl-netd --socket <path> (--control-fd N | --unguarded)
//! [--profile <dir>] [--clients N] [--in-flight N] [--requests N]
//! [--body-bytes N]`: the network process. It listens on the Unix socket
//! at `path`, replacing a stale socket file there, and serves until
//! killed. `--control-fd` is the inherited end of the socket pair its host
//! registers documents on; `--unguarded` trusts every client's claims and
//! is for tests and tools only. With a profile, its cookies are loaded at
//! start and saved as they change and on SIGTERM, SIGINT or SIGHUP, and
//! downloads go to its `downloads` directory; a profile another process
//! holds is refused.

mod stop;

use madcawl_net::{Client, Config};
use madcawl_netd::{Downloads, Profile, ServerLimits, serve_guarded, serve_with};
use std::os::fd::FromRawFd;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

unsafe extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

const F_GETFD: i32 = 1;

const USAGE: &str = "usage: madcawl-netd --socket <path> (--control-fd N | --unguarded) [--profile <dir>] [--clients N] \
                     [--in-flight N] [--requests N] [--body-bytes N]";

/// Opens the profile, if one is given, and has SIGTERM, SIGINT and SIGHUP
/// save it and remove the socket before the process exits.
fn open_profile(dir: Option<PathBuf>, socket: &str, client: &Arc<Client>, signals: stop::SigSet) -> Result<Option<Arc<Downloads>>, ()> {
    let Some(dir) = dir else {
        stop::on_signal(signals, || std::process::exit(0));
        return Ok(None);
    };
    let p = Profile::open(&dir, client.clone()).map_err(|e| eprintln!("madcawl-netd: {e}"))?;
    let d = p.downloads();
    let socket = socket.to_owned();
    stop::on_signal(signals, move || {
        drop(p);
        let _ = std::fs::remove_file(&socket);
        std::process::exit(0);
    });
    Ok(Some(d))
}

fn main() -> ExitCode {
    let (mut socket, mut profile) = (None, None);
    let (mut control_fd, mut unguarded) = (None, false);
    let mut limits = ServerLimits::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--unguarded" {
            unguarded = true;
            continue;
        }
        let value = args.next();
        let number = value.as_deref().and_then(|v| v.parse::<usize>().ok());
        match (a.as_str(), value, number) {
            ("--socket", Some(v), _) => socket = Some(v),
            ("--profile", Some(v), _) => profile = Some(PathBuf::from(v)),
            ("--control-fd", _, Some(n)) if n > 2 && i32::try_from(n).is_ok() => control_fd = Some(n as i32),
            ("--clients", _, Some(n)) => limits.clients = n,
            ("--in-flight", _, Some(n)) => limits.in_flight = n,
            ("--requests", _, Some(n)) => limits.requests = n,
            ("--body-bytes", _, Some(n)) => limits.body_bytes = n,
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(path) = socket else {
        eprintln!("madcawl-netd: --socket is required");
        return ExitCode::from(2);
    };
    if control_fd.is_some() == unguarded {
        eprintln!("madcawl-netd: exactly one of --control-fd and --unguarded is required\n{USAGE}");
        return ExitCode::from(2);
    }
    // taken before the socket is bound, so that a descriptor the host did not
    // pass cannot be handed to the listener and then owned twice
    let control = match control_fd.map(control_channel).transpose() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("madcawl-netd: control channel: {e}");
            return ExitCode::FAILURE;
        }
    };
    let signals = stop::block();
    let client = Arc::new(Client::new(Config::default()));
    let Ok(downloads) = open_profile(profile, &path, &client, signals) else { return ExitCode::FAILURE };
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_socket()) {
        let _ = std::fs::remove_file(&path);
    }
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("madcawl-netd: bind {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(control) = control else {
        serve_with(listener.incoming(), client, limits, downloads);
        return ExitCode::SUCCESS;
    };
    match serve_guarded(listener, control, client, limits, downloads) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("madcawl-netd: control channel: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The control channel on `fd`, which must be open and a connected socket.
fn control_channel(fd: i32) -> std::io::Result<UnixStream> {
    // SAFETY: F_GETFD only reads the descriptor's flags
    if unsafe { fcntl(fd, F_GETFD) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the descriptor is open, the host passed it for the control channel and nothing else here owns it
    let control = unsafe { UnixStream::from_raw_fd(fd) };
    control.peer_addr()?;
    Ok(control)
}
