//! Each session's pages in a content process of their own (`--isolate`):
//! killing one reloads its pages where they were and touches no other
//! session; a process that stops answering or goes over its memory cap
//! is recovered the same way; one that keeps crashing is braked; frames
//! of the host's own are refused from agents; an ended session's process
//! exits, as every child does when the host is killed; children keep no
//! descriptor the host did not hand them.

mod isolate;

use isolate::{Fixture, SIGKILL, SIGSTOP, Session, alive, isolated, long_page, signal, until};
use madcawl_agent::{Frame, kind, tag};
use madcawl_shell::rusage::fds;
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn pipe(fds: *mut i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    fn close(fd: i32) -> i32;
}

const CRASHED: u64 = 24;
const HUNG: u64 = 25;

#[test]
fn a_killed_content_process_has_its_pages_back_where_they_were_within_a_second() {
    let f = isolated(false, |_| {});
    let long = long_page(&f);
    let mut s = Session::new(&f);
    let p = s.open(&long);
    let at = s.scroll(p, 2);
    assert!(at.contains("at y=1600"), "{at}");
    let before = s.read(p).unwrap();
    let first = s.stat("content-pid") as i32;
    let t = Instant::now();
    signal(first, SIGKILL);
    // the host notices at once, with no request open, and restores eagerly
    assert!(until(5, || !alive(first)));
    let restored = until(5, || s.stat("content-restarts") == 1);
    let took = t.elapsed();
    assert!(restored, "a new content process");
    assert!(took < Duration::from_secs(1), "back within a second: {took:?}");
    assert_ne!(s.stat("content-pid") as i32, first);
    assert_eq!(s.read(p).unwrap(), before, "the same page, scrolled to the same place");
    let again = s.scroll(p, 0);
    assert!(again.contains("at y=1600"), "{again}");
}

#[test]
fn killing_one_sessions_process_leaves_another_session_alone() {
    let f = isolated(false, |_| {});
    let page = f.page();
    let (mut a, mut b) = (Session::new(&f), Session::new(&f));
    let (pa, pb) = (a.open(&page), b.open(&page));
    let b_pid = b.stat("content-pid") as i32;
    let a_pid = a.stat("content-pid") as i32;
    assert_ne!(a_pid, b_pid, "a process each");
    signal(a_pid, SIGKILL);
    let b_text = b.read(pb).unwrap();
    assert!(b_text.contains("zebra-secret-paragraph"), "{b_text}");
    assert_eq!(b.stat("content-pid") as i32, b_pid);
    assert_eq!(b.stat("content-restarts"), 0);
    assert!(until(5, || a.read(pa).is_ok_and(|t| t == b_text)));
    let denied = b.read(pa).unwrap_err();
    assert_eq!(denied.0, 21, "another session's page stays refused: {denied:?}");
}

#[test]
fn a_content_process_that_stops_answering_is_killed_after_the_hang_time() {
    let f = isolated(false, |f| f.config.hang = Duration::from_secs(2));
    let page = f.page();
    let mut s = Session::new(&f);
    let p = s.open(&page);
    let pid = s.stat("content-pid") as i32;
    signal(pid, SIGSTOP);
    let t = Instant::now();
    let (code, text) = s.read(p).unwrap_err();
    assert_eq!(code, HUNG, "{text}");
    assert!(t.elapsed() >= Duration::from_secs(2) && t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
    assert!(text.contains(&format!("p{p} at y=0")), "{text}");
    assert!(!alive(pid), "the stopped process was killed");
    assert!(s.read(p).unwrap().contains("zebra-secret-paragraph"));
}

#[test]
fn a_content_process_over_its_memory_cap_exits_and_its_other_pages_come_back() {
    let f = isolated(false, |f| f.config.memory_cap = Some(96 << 20));
    let small = f.page();
    let mut big = String::from("<!doctype html><title>Big</title>");
    for i in 0..300_000 {
        big.push_str(&format!("<div class=c{}><span>cell {i}</span></div>", i % 97));
    }
    let big_path = f.dir.join("big.html");
    std::fs::write(&big_path, big).unwrap();
    let mut s = Session::new(&f);
    let p = s.open(&small);
    let r = s.ask(Frame::new(kind::OPEN).str(tag::URL, big_path.to_str().unwrap()));
    let text = r.get_str(tag::TEXT).unwrap_or("");
    assert_eq!((r.kind, r.get_num(tag::CODE)), (kind::ERROR, Some(CRASHED)), "{text}");
    assert!(text.contains("memory cap") && text.contains(&format!("p{p} at y=0")), "{text}");
    assert!(s.read(p).unwrap().contains("zebra-secret-paragraph"));
    let pages = s.ask(Frame::new(kind::PAGES));
    assert_eq!(pages.get_str(tag::TEXT).unwrap().lines().count(), 1, "the big page is not opened again");
}

#[test]
fn a_content_process_that_keeps_crashing_is_not_started_again_for_its_pages() {
    let f = isolated(false, |_| {});
    let page = f.page();
    let mut s = Session::new(&f);
    let p = s.open(&page);
    for n in 1..=2 {
        let pid = s.stat("content-pid") as i32;
        signal(pid, SIGKILL);
        assert!(until(5, || s.stat("content-restarts") == n));
    }
    let pid = s.stat("content-pid") as i32;
    signal(pid, SIGKILL);
    assert!(until(5, || !alive(pid)));
    let (code, text) = s.read(p).unwrap_err();
    assert_eq!(code, CRASHED, "{text}");
    assert!(text.contains("dropped"), "{text}");
    let q = s.open(&page);
    assert!(s.read(q).unwrap().contains("zebra-secret-paragraph"), "opening a page again works");
}

#[test]
fn frames_of_the_hosts_own_are_refused_from_an_agent() {
    let f = isolated(false, |_| {});
    let mut s = Session::new(&f);
    for k in [240u16, 241, 242, 243, 244, 245, 246] {
        let r = s.ask(Frame::new(k).str(tag::URL, &f.page()).num(tag::PAGE, 1));
        assert_eq!((r.kind, r.get_num(tag::CODE)), (kind::ERROR, Some(6)), "kind {k}");
    }
    let p = s.open(&f.page());
    assert!(s.read(p).is_ok(), "the session goes on");
}

#[test]
fn an_ended_sessions_content_process_exits() {
    let f = isolated(false, |_| {});
    let mut s = Session::new(&f);
    s.open(&f.page());
    let pid = s.stat("content-pid") as i32;
    drop(s);
    assert!(until(5, || !alive(pid)), "the content process exits with its session");
    let audit = f.audit();
    assert!(audit.contains("\tclose-session\t"), "{audit}");
}

/// A descriptor of the test process's, numbered 200 or above, that every
/// child it starts inherits unless the child closes it.
fn leaked_fd() -> i32 {
    const F_DUPFD: i32 = 0;
    let mut p = [0i32; 2];
    // SAFETY: pipe fills two descriptors; F_DUPFD copies one without close-on-exec; the originals are closed
    unsafe {
        assert_eq!(pipe(p.as_mut_ptr()), 0);
        let high = fcntl(p[0], F_DUPFD, 200);
        assert!(high >= 200, "{high}");
        close(p[0]);
        close(p[1]);
        high
    }
}

#[test]
fn content_and_network_processes_keep_no_descriptor_of_the_hosts() {
    let leaked = leaked_fd();
    let f = isolated(true, |_| {});
    let mut s = Session::new(&f);
    s.open(&f.page());
    let mut control = std::process::Command::new("/bin/sleep").arg("10").spawn().unwrap();
    let theirs = fds(control.id() as i32).unwrap();
    let _ = control.kill();
    let _ = control.wait();
    assert!(theirs.contains(&leaked), "the probe sees an inherited descriptor: {theirs:?}");
    for name in ["content-pid", "netd-pid"] {
        let list = fds(s.stat(name) as i32).unwrap();
        assert!(list.starts_with(&[0, 1, 2]) && list.iter().all(|&fd| fd < 200), "{name}: {list:?}");
    }
}

#[test]
fn killing_the_host_ends_its_content_and_network_processes() {
    let f = Fixture::new();
    let mut host = std::process::Command::new(env!("CARGO_BIN_EXE_madcawl-shell"))
        .arg("--serve")
        .arg(&f.config.socket)
        .args(["--isolate", "--stats", "--net", "--allow"])
        .arg(&f.dir)
        .arg("--audit")
        .arg(f.dir.join("audit.log"))
        .spawn()
        .unwrap();
    assert!(until(10, || f.config.socket.exists()), "the host listens");
    let mut s = Session::new(&f);
    s.open(&f.page());
    let children = [s.stat("content-pid") as i32, s.stat("netd-pid") as i32];
    assert!(children.iter().all(|&p| alive(p)));
    signal(host.id() as i32, SIGKILL);
    host.wait().unwrap();
    assert!(until(5, || children.iter().all(|&p| !alive(p))), "children {children:?} outlived the host");
}
