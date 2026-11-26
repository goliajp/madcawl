//! The network service in a process of its own (`--isolate`), which the
//! host starts again when it ends: pages loaded before stay as they
//! were, later loads go to the new process, each session with pages
//! from the network is told once that its cookies went with the old
//! process, and a process that keeps ending is not started a fourth time
//! within a minute.

mod isolate;

use isolate::act_site::{Mcp, page_of, path_arg, site};
use isolate::{Fixture, SIGKILL, Session, isolated, signal, until};
use std::time::Duration;

fn stat(f: &Fixture, name: &str) -> i64 {
    Session::new(f).stat(name)
}

/// A reply's page text, without the envelope lines.
fn content(reply: &str) -> String {
    reply.lines().filter(|l| !l.contains("page-content-")).collect::<Vec<_>>().join("\n")
}

fn read(m: &mut Mcp, page: &str) -> String {
    content(&m.ok("read", &format!(r#"{{"page":"{page}"}}"#)))
}

#[test]
fn killing_the_network_process_keeps_loaded_pages_and_later_loads_work() {
    let origin = site();
    let f = isolated(true, |_| {});
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/"))));
    let before = read(&mut m, &p);
    assert!(before.contains("Site"), "{before}");
    let first = stat(&f, "netd-pid") as i32;
    signal(first, SIGKILL);
    assert!(until(10, || stat(&f, "netd-restarts") == 1), "the network process is started again");
    let second = stat(&f, "netd-pid") as i32;
    assert_ne!(first, second);
    let after = read(&mut m, &p);
    let told = |l: &str| l.contains("network process was restarted") && l.contains("cookies");
    assert_eq!(after.lines().filter(|l| told(l)).count(), 1, "the session is told its cookies went: {after}");
    let rest: Vec<&str> = after.lines().filter(|l| !told(l)).collect();
    assert_eq!(rest.join("\n"), before, "a page loaded before the crash is as it was");
    assert_eq!(read(&mut m, &p), before, "told once");
    let login = m.ok("open", &path_arg(&format!("{origin}/login")));
    assert!(login.contains("Log in"), "a load after the crash goes to the new process: {login}");
}

#[test]
fn a_page_whose_connection_died_with_the_network_process_navigates_again() {
    let origin = site();
    let f = isolated(true, |_| {});
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
    let f = isolated(true, |_| {});
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
