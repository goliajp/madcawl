//! What a host is configured with, and the `--serve` command line that
//! gives it.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Config {
    pub socket: PathBuf,
    pub audit: PathBuf,
    /// Only files below these canonical directories open, page and
    /// stylesheet alike; none means nothing opens
    pub allow: Vec<PathBuf>,
    pub max_sessions: usize,
    pub max_pages: usize,
    /// Pages open across every session
    pub max_total_pages: usize,
    /// A session or page nobody asked anything of for this long closes
    pub idle: Duration,
    pub max_request: usize,
    pub max_reply: usize,
    /// Requests a second a session may make, and how many it may make
    /// at once after a quiet spell
    pub rate: f64,
    pub burst: f64,
    /// An inherited, already listening socket to accept on instead of
    /// binding the socket path
    pub listen_fd: Option<i32>,
    /// With no session open for this long, the process exits
    pub exit_idle: Option<Duration>,
}

impl Config {
    /// The defaults; the only allowed directory is the current one.
    pub fn new(socket: PathBuf) -> Config {
        let mut audit = socket.clone().into_os_string();
        audit.push(".audit.log");
        let cwd = std::env::current_dir().and_then(|d| d.canonicalize());
        Config {
            socket,
            audit: audit.into(),
            allow: cwd.into_iter().collect(),
            max_sessions: 16,
            max_pages: 64,
            max_total_pages: 64,
            idle: Duration::from_secs(600),
            max_request: 64 << 10,
            max_reply: 4 << 20,
            rate: 50.0,
            burst: 100.0,
            listen_fd: None,
            exit_idle: None,
        }
    }
}

pub const USAGE: &str = "madcawl-shell --serve <socket> [--allow <dir>]... [--audit <file>] [--sessions N] [--pages N] [--total-pages N] [--idle <seconds>] [--listen-fd N] [--exit-idle <seconds>]
         pages and their stylesheets open only from below the --allow directories;
         without --allow, only from below the current directory;
         --listen-fd accepts on an inherited listening socket instead of binding <socket>;
         --exit-idle exits once no session has been open for that long (checked once a period
         while sessions are open, so up to twice that after the last one ends)";

/// The configuration the `--serve` command line gives.
pub fn parse_args(args: &[&str]) -> Option<Config> {
    let [socket, rest @ ..] = args else {
        return None;
    };
    if socket.starts_with('-') {
        return None;
    }
    let mut c = Config::new(PathBuf::from(socket));
    let mut allow = Vec::new();
    let mut rest = rest.iter();
    while let Some(&opt) = rest.next() {
        let v = *rest.next()?;
        let n = || v.parse::<usize>().ok().filter(|&n| n > 0);
        match opt {
            "--allow" => allow.push(PathBuf::from(v).canonicalize().ok()?),
            "--audit" => c.audit = PathBuf::from(v),
            "--sessions" => c.max_sessions = n()?,
            "--pages" => c.max_pages = n()?,
            "--total-pages" => c.max_total_pages = n()?,
            "--idle" => c.idle = Duration::from_secs(n()? as u64),
            "--listen-fd" => c.listen_fd = Some(v.parse::<i32>().ok().filter(|&fd| fd > 2)?),
            "--exit-idle" => c.exit_idle = Some(Duration::from_secs(n()? as u64)),
            _ => return None,
        }
    }
    if !allow.is_empty() {
        c.allow = allow;
    }
    Some(c)
}
