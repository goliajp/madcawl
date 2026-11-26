//! Crash isolation and recovery with `--isolate`: the network service
//! in a process of its own, which the host starts again when it ends.

mod act_site;

use act_site::{Fixture, Mcp, page_of, path_arg, site};
use madcawl_agent::{Frame, kind, tag};
use std::path::PathBuf;
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

const SIGKILL: i32 = 9;

fn signal(pid: i32, sig: i32) {
    assert!(pid > 1);
    // SAFETY: kill only sends a signal to a process the test started
    assert_eq!(unsafe { kill(pid, sig) }, 0, "signal {sig} to {pid}");
}

/// A host with the network on and `--isolate`, answering STATS; started.
fn isolated(net: bool) -> Fixture {
    let mut f = Fixture::new();
    f.config.net = net;
    f.config.downloads = net.then(|| f.dir.join("downloads"));
    f.config.isolate = Some(PathBuf::from(env!("CARGO_BIN_EXE_madcawl-shell")));
    f.config.stats = true;
    f.start();
    f
}

/// One of the host's counters, read on a connection of its own.
fn stat(f: &Fixture, name: &str) -> i64 {
    let r = act_site::raw(f, &act_site::EncodeVec::encode_vec(&Frame::new(kind::STATS)));
    let text = r.get_str(tag::TEXT).unwrap_or_else(|| panic!("no counters: {r:?}")).to_owned();
    let line = text.lines().find(|l| l.split(' ').next() == Some(name));
    line.and_then(|l| l.split(' ').nth(1)?.parse().ok()).unwrap_or_else(|| panic!("no {name} in {text}"))
}

/// A reply's page text, without the envelope lines, whose markers are
/// new on every reply.
fn content(reply: &str) -> String {
    reply.lines().filter(|l| !l.contains("page-content-")).collect::<Vec<_>>().join("\n")
}

fn read(m: &mut Mcp, page: &str) -> String {
    content(&m.ok("read", &format!(r#"{{"page":"{page}"}}"#)))
}

/// Polls `f` until it holds or `secs` pass.
fn until(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn killing_the_network_process_keeps_loaded_pages_and_later_loads_work() {
    let origin = site();
    let f = isolated(true);
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/"))));
    let before = read(&mut m, &p);
    assert!(before.contains("Site"), "{before}");
    let first = stat(&f, "netd-pid") as i32;
    signal(first, SIGKILL);
    assert!(until(10, || stat(&f, "netd-restarts") == 1), "the network process is started again");
    let second = stat(&f, "netd-pid") as i32;
    assert_ne!(first, second);
    assert_eq!(read(&mut m, &p), before, "a page loaded before the crash is as it was");
    let login = m.ok("open", &path_arg(&format!("{origin}/login")));
    assert!(login.contains("Log in"), "a load after the crash goes to the new process: {login}");
}

#[test]
fn a_page_whose_connection_died_with_the_network_process_navigates_again() {
    let origin = site();
    let f = isolated(true);
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/"))));
    signal(stat(&f, "netd-pid") as i32, SIGKILL);
    assert!(until(10, || stat(&f, "netd-restarts") == 1));
    let args = format!(r#"{{"page":"{p}","action":"navigate","value":"{origin}/login"}}"#);
    let (t, err) = m.call("act", &args);
    assert!(!err && t.contains("navigated"), "{t}");
}

#[test]
fn the_network_process_is_not_started_a_fourth_time_within_a_minute() {
    let f = isolated(true);
    for n in 1..=2 {
        signal(stat(&f, "netd-pid") as i32, SIGKILL);
        assert!(until(10, || stat(&f, "netd-restarts") == n));
    }
    let last = stat(&f, "netd-pid") as i32;
    signal(last, SIGKILL);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(stat(&f, "netd-restarts"), 2, "the third end within a minute starts no other");
    let origin = site();
    let (t, err) = f.mcp().call("open", &path_arg(&format!("{origin}/")));
    assert!(err && t.contains("network"), "{t}");
}
