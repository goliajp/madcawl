//! What the crash isolation and recovery tests share: a host started
//! with `--isolate` (each session's pages in a content process, the
//! network service in a process of its own), a session spoken to in raw
//! frames, and signals to the host's processes.
#![allow(dead_code)]

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use madcawl_agent::{Frame, kind, tag};

#[path = "../act_site/mod.rs"]
pub mod act_site;
pub use act_site::{EncodeVec, Fixture, exchange};

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

pub const SIGKILL: i32 = 9;
pub const SIGSTOP: i32 = 17;

pub fn signal(pid: i32, sig: i32) {
    assert!(pid > 1);
    // SAFETY: kill only sends a signal, to a process the test's host started
    assert_eq!(unsafe { kill(pid, sig) }, 0, "signal {sig} to {pid}");
}

/// Whether process `pid` is still there.
pub fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists
    unsafe { kill(pid, 0) == 0 }
}

/// A host with `--isolate`, answering STATS, the network on when `net`;
/// `with` changes its configuration before it starts.
pub fn isolated(net: bool, with: impl FnOnce(&mut Fixture)) -> Fixture {
    let mut f = Fixture::new();
    f.config.net = net;
    f.config.downloads = net.then(|| f.dir.join("downloads"));
    f.config.isolate = Some(PathBuf::from(env!("CARGO_BIN_EXE_madcawl-shell")));
    f.config.stats = true;
    with(&mut f);
    f.start();
    f
}

/// Polls `f` until it holds or `secs` pass.
pub fn until(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// One agent session in raw frames.
pub struct Session(pub UnixStream);

impl Session {
    pub fn new(f: &Fixture) -> Session {
        Session(UnixStream::connect(&f.config.socket).unwrap())
    }

    pub fn ask(&mut self, f: Frame) -> Frame {
        exchange(&mut self.0, &f.encode_vec())
    }

    /// Opens `path`; the page number.
    pub fn open(&mut self, path: &str) -> u64 {
        let r = self.ask(Frame::new(kind::OPEN).str(tag::URL, path));
        assert_eq!(r.kind, kind::OPENED, "{:?}", r.get_str(tag::TEXT));
        r.get_num(tag::PAGE).unwrap()
    }

    /// The page's text, without the envelope lines, whose markers are new
    /// on every reply; or the error.
    pub fn read(&mut self, page: u64) -> Result<String, (u64, String)> {
        let r = self.ask(Frame::new(kind::READ).num(tag::PAGE, page).num(tag::VIEW, 2));
        let text = r.get_str(tag::TEXT).unwrap_or("").to_owned();
        match r.kind {
            kind::ERROR => Err((r.get_num(tag::CODE).unwrap_or(0), text)),
            _ => Ok(text.lines().filter(|l| !l.contains("page-content-")).collect::<Vec<_>>().join("\n")),
        }
    }

    pub fn scroll(&mut self, page: u64, viewports: u64) -> String {
        let f = Frame::new(kind::ACT).num(tag::PAGE, page).str(tag::ACTION, "scroll").str(tag::VALUE, "down").num(tag::AMOUNT, viewports);
        self.ask(f).get_str(tag::TEXT).unwrap_or("").to_owned()
    }

    /// One of the host's counters for this session, asked again when the
    /// request met a content process that had just been killed.
    pub fn stat(&mut self, name: &str) -> i64 {
        let mut r = self.ask(Frame::new(kind::STATS));
        if r.get_num(tag::CODE) == Some(24) {
            r = self.ask(Frame::new(kind::STATS));
        }
        let text = r.get_str(tag::TEXT).unwrap_or_else(|| panic!("no counters: {r:?}")).to_owned();
        let line = text.lines().find(|l| l.split(' ').next() == Some(name));
        line.and_then(|l| l.split(' ').nth(1)?.parse().ok()).unwrap_or_else(|| panic!("no {name} in {text}"))
    }
}

/// A page tall enough to scroll five viewports, in the fixture's
/// directory; its path.
pub fn long_page(f: &Fixture) -> String {
    let mut html = String::from("<!doctype html><title>Long</title><h1>Long page</h1>");
    for i in 0..400 {
        html.push_str(&format!("<p>Paragraph {i} of the long page.</p>"));
    }
    let p = f.dir.join("long.html");
    std::fs::write(&p, html).unwrap();
    p.display().to_string()
}
