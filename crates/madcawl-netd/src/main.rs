//! `madcawl-netd --socket <path> [--clients N] [--in-flight N] [--requests N]`: the
//! network process. It listens on the Unix socket at `path`, replacing a
//! stale socket file there, and serves until killed.

use madcawl_net::{Client, Config};
use madcawl_netd::{ServerLimits, serve};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixListener;
use std::process::ExitCode;
use std::sync::Arc;

fn main() -> ExitCode {
    let mut socket = None;
    let mut limits = ServerLimits::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let value = args.next();
        let number = value.as_deref().and_then(|v| v.parse::<usize>().ok());
        match (a.as_str(), value, number) {
            ("--socket", Some(v), _) => socket = Some(v),
            ("--clients", _, Some(n)) => limits.clients = n,
            ("--in-flight", _, Some(n)) => limits.in_flight = n,
            ("--requests", _, Some(n)) => limits.requests = n,
            _ => {
                eprintln!("usage: madcawl-netd --socket <path> [--clients N] [--in-flight N] [--requests N]");
                return ExitCode::from(2);
            }
        }
    }
    let Some(path) = socket else {
        eprintln!("madcawl-netd: --socket is required");
        return ExitCode::from(2);
    };
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
    serve(listener, Arc::new(Client::new(Config::default())), limits);
    ExitCode::SUCCESS
}
