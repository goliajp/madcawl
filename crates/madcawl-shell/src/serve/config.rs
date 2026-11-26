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
    /// Pages may come from the network, through the host's network
    /// service, each session's in a cookie jar of the session's own that
    /// starts empty and is dropped when the session ends
    pub net: bool,
    /// Where downloads go, a directory per session below it
    pub downloads: Option<PathBuf>,
    /// The largest download; the directory takes four times it
    pub download_cap: u64,
    /// Files an upload may pick come only from below these canonical
    /// directories, which the user approved by starting the host
    pub uploads: Vec<PathBuf>,
    /// Origins a form with a password field may be submitted to
    pub approve: Vec<String>,
    /// answer `STATS` requests: what the process holds for all its
    /// sessions, so one session would learn of the others' pages
    pub stats: bool,
    /// Time each request of a session and log one that runs longer than
    /// this, naming the stage it is in (see `watch`)
    pub watch: Option<Duration>,
    /// The memory cap of the process the session's pages are in (see
    /// `memory`); over it the process exits
    pub memory_cap: Option<u64>,
    /// Run the network service in a process of its own, this program
    /// started with `--netd`, and start it again when it ends
    pub isolate: Option<PathBuf>,
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
            net: false,
            downloads: None,
            download_cap: 256 << 20,
            uploads: Vec::new(),
            approve: Vec::new(),
            stats: false,
            watch: None,
            memory_cap: None,
            isolate: None,
        }
    }
}

pub const USAGE: &str = "madcawl-shell --serve <socket> [--allow <dir>]... [--audit <file>] [--sessions N] [--pages N] [--total-pages N] [--idle <seconds>] [--listen-fd N] [--exit-idle <seconds>] [--net] [--downloads <dir>] [--upload-allow <dir>]... [--approve <origin>]... [--stats] [--watch-ms N] [--memory-cap-mib N] [--isolate]
         pages and their stylesheets open only from below the --allow directories;
         without --allow, only from below the current directory;
         --net lets pages come from http and https URLs, each session's in a cookie jar of its own that starts empty and ends with it;
         --downloads saves downloads below <dir>, a directory per session (default <socket>.downloads);
         --upload-allow lets an upload pick files from below <dir>; none, no uploads;
         --approve lets a form with a password field be submitted to <origin> (scheme://host[:port]);
         --watch-ms logs a request still running after N ms, and once more when it runs ten times as
         long (at least 10 s), naming the stage it is in, and a slow request's time by stage when it ends;
         --memory-cap-mib drops the stylesheet cache past three quarters of N MiB of footprint and exits
         past N (a host without --isolate holds every session's pages and exits with them);
         --isolate runs the network service in a process of its own and starts it again when it ends;
         --stats answers the host's own counters (shared stylesheet cache, pages open) to any session;
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
        if opt == "--isolate" {
            c.isolate = Some(std::env::current_exe().ok()?);
            continue;
        }
        let flag = match opt {
            "--net" => Some(&mut c.net),
            "--stats" => Some(&mut c.stats),
            _ => None,
        };
        if let Some(f) = flag {
            *f = true;
            continue;
        }
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
            "--downloads" => c.downloads = Some(PathBuf::from(v)),
            "--upload-allow" => c.uploads.push(PathBuf::from(v).canonicalize().ok()?),
            "--memory-cap-mib" => c.memory_cap = Some(n()? as u64 * (1 << 20)),
            "--watch-ms" => c.watch = Some(Duration::from_millis(n()? as u64)),
            "--approve" => c.approve.push(v.trim_end_matches('/').to_ascii_lowercase()),
            _ => return None,
        }
    }
    if !allow.is_empty() {
        c.allow = allow;
    }
    if c.net && c.downloads.is_none() {
        let mut d = c.socket.clone().into_os_string();
        d.push(".downloads");
        c.downloads = Some(d.into());
    }
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_network_and_the_users_approvals_come_from_the_command_line() {
        let tmp = std::env::temp_dir();
        let up = tmp.to_str().unwrap();
        let c = parse_args(&["/s", "--net", "--upload-allow", up, "--approve", "HTTP://127.0.0.1:8/"]).unwrap();
        assert!(c.net);
        assert_eq!(c.downloads, Some(PathBuf::from("/s.downloads")), "the default beside the socket");
        assert_eq!(c.uploads, [tmp.canonicalize().unwrap()]);
        assert_eq!(c.approve, ["http://127.0.0.1:8"]);
        let c = parse_args(&["/s", "--downloads", "/d"]).unwrap();
        assert!(!c.net && !c.stats && c.uploads.is_empty() && c.approve.is_empty());
        assert!(parse_args(&["/s", "--stats"]).unwrap().stats);
        assert_eq!(c.downloads, Some(PathBuf::from("/d")));
        assert!(parse_args(&["/s", "--upload-allow", "/no/such/dir"]).is_none());
    }
}
