//! The host started on a socket its parent bound: it serves the
//! connection that woke the parent, leaves once idle, and what arrives
//! after it left waits in the queue for the next host.

mod serve_fixture;

use std::os::fd::AsRawFd as _;
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt as _;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use madcawl_shell::serve::parse_args;
use serve_fixture::{Fixture, path_arg};

unsafe extern "C" {
    fn dup2(from: i32, to: i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

/// `fd` at 3 in the child, open across exec. dup2 onto itself keeps
/// close-on-exec set, so a socket already at 3 has the flag cleared.
fn at_three(cmd: &mut Command, fd: i32) {
    // SAFETY: dup2 and fcntl are async-signal-safe
    unsafe {
        cmd.pre_exec(move || {
            let ok = if fd == 3 { fcntl(3, 2, 0) == 0 } else { dup2(fd, 3) == 3 };
            if ok { Ok(()) } else { Err(std::io::Error::last_os_error()) }
        });
    }
}

fn host(f: &Fixture, listener: &UnixListener, exit_idle: u64) -> Child {
    let fd = listener.as_raw_fd();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_madcawl-shell"));
    cmd.arg("--serve")
        .arg(&f.config.socket)
        .args(["--listen-fd", "3", "--exit-idle", &exit_idle.to_string()])
        .arg("--allow")
        .arg(&f.dir)
        .stdin(Stdio::null());
    at_three(&mut cmd, fd);
    cmd.spawn().unwrap()
}

fn exits_within(child: &mut Child, limit: Duration) -> Option<std::process::ExitStatus> {
    let t = Instant::now();
    while t.elapsed() < limit {
        if let Some(s) = child.try_wait().unwrap() {
            return Some(s);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
fn an_activated_host_serves_leaves_when_idle_and_the_queue_waits_for_the_next() {
    let f = Fixture::new();
    let listener = UnixListener::bind(&f.config.socket).unwrap();
    let mut first = host(&f, &listener, 1);
    {
        let mut m = f.mcp();
        assert!(m.ok("open", &path_arg(&f.page())).starts_with("opened p1"));
        // a session keeps the host however long it lasts
        std::thread::sleep(Duration::from_millis(1500));
        assert!(first.try_wait().unwrap().is_none(), "left with a session open");
        m.ok("pages", "{}");
    }
    let status = exits_within(&mut first, Duration::from_secs(5)).expect("left once idle");
    assert!(status.success(), "{status}");
    assert!(f.config.socket.exists(), "the parent's socket stays");

    // nobody accepts now: a request made before the next host starts
    // waits in the queue and is answered by it
    let mut early = std::os::unix::net::UnixStream::connect(&f.config.socket).unwrap();
    let ask = madcawl_agent::Frame::new(madcawl_agent::kind::PAGES);
    std::io::Write::write_all(&mut early, &serve_fixture::EncodeVec::encode_vec(&ask)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let mut second = host(&f, &listener, 1);
    let reply = serve_fixture::exchange(&mut early, &[]);
    assert_ne!(reply.kind, madcawl_agent::kind::ERROR, "{reply:?}");
    drop(early);
    assert!(exits_within(&mut second, Duration::from_secs(5)).is_some_and(|s| s.success()));
}

#[test]
fn a_host_never_used_leaves_after_the_idle_time() {
    let f = Fixture::new();
    let listener = UnixListener::bind(&f.config.socket).unwrap();
    let mut h = host(&f, &listener, 1);
    let t = Instant::now();
    let status = exits_within(&mut h, Duration::from_secs(5)).expect("left");
    assert!(status.success() && t.elapsed() >= Duration::from_millis(900), "{status} {:?}", t.elapsed());
}

#[test]
fn an_inherited_descriptor_that_does_not_listen_is_refused() {
    let f = Fixture::new();
    let file = std::fs::File::open(f.page()).unwrap();
    let fd = file.as_raw_fd();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_madcawl-shell"));
    cmd.arg("--serve").arg(&f.config.socket).args(["--listen-fd", "3"]).stderr(Stdio::piped());
    at_three(&mut cmd, fd);
    let out = cmd.output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a socket"), "{out:?}");
}

#[test]
fn an_inherited_socket_that_does_not_listen_ends_the_host() {
    let f = Fixture::new();
    let (a, mut b) = std::os::unix::net::UnixStream::pair().unwrap();
    // readable, so the host tries to accept on it at once
    std::io::Write::write_all(&mut b, b"x").unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_madcawl-shell"));
    cmd.arg("--serve").arg(&f.config.socket).args(["--listen-fd", "3", "--exit-idle", "30"]).stderr(Stdio::piped());
    at_three(&mut cmd, a.as_raw_fd());
    let mut child = cmd.spawn().unwrap();
    let status = exits_within(&mut child, Duration::from_secs(5)).expect("ended rather than spun");
    assert!(!status.success());
}

#[test]
fn the_activation_flags_parse_and_bad_values_are_refused() {
    let c = parse_args(&["/s", "--listen-fd", "3", "--exit-idle", "300"]).unwrap();
    assert_eq!(c.listen_fd, Some(3));
    assert_eq!(c.exit_idle, Some(Duration::from_secs(300)));
    let plain = parse_args(&["/s"]).unwrap();
    assert_eq!((plain.listen_fd, plain.exit_idle), (None, None));
    for bad in [["--listen-fd", "1"], ["--listen-fd", "x"], ["--exit-idle", "0"]] {
        assert!(parse_args(&["/s", bad[0], bad[1]]).is_none(), "{bad:?}");
    }
}
