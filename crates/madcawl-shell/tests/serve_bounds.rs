//! What the host refuses or bounds: files that are not regular files,
//! stylesheets outside the allowed directories, an unsafe socket path,
//! idle sessions, request floods, the host-wide page count, file names
//! that would break a line; and the two frame codecs agreeing.

mod serve_fixture;

use std::io::Read as _;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use madcawl_agent::{Frame, kind, tag};
use madcawl_shell::load::read_file;
use madcawl_shell::serve::{Server, code};
use serve_fixture::{EncodeVec as _, Fixture, path_arg};

fn mkfifo(p: &Path) {
    let ok = std::process::Command::new("mkfifo")
        .arg(p)
        .status()
        .unwrap();
    assert!(ok.success());
}

#[test]
fn only_bounded_regular_files_are_read() {
    let f = Fixture::new();
    let t = Instant::now();
    assert!(read_file(Path::new("/dev/zero"), 1 << 20).is_err());
    let fifo = f.dir.join("fifo.html");
    mkfifo(&fifo);
    assert!(read_file(&fifo, 1 << 20).is_err());
    assert!(t.elapsed() < Duration::from_secs(5), "nothing blocked");
    let big = f.dir.join("big.html");
    std::fs::write(&big, vec![b'a'; 101]).unwrap();
    assert!(read_file(&big, 100).is_err());
    assert_eq!(read_file(&big, 101).unwrap().len(), 101);
}

#[test]
fn the_host_refuses_devices_and_fifos() {
    let mut f = Fixture::new();
    f.config
        .allow
        .push(Path::new("/dev").canonicalize().unwrap());
    let fifo = f.dir.join("fifo.html");
    mkfifo(&fifo);
    f.start();
    let mut m = f.mcp();
    for p in ["/dev/zero".to_owned(), fifo.display().to_string()] {
        let (t, err) = m.call("open", &path_arg(&p));
        assert!(err && t.contains("not a regular file"), "{p}: {t}");
    }
}

#[test]
fn stylesheets_outside_the_allowed_directories_are_not_read() {
    let mut f = Fixture::new();
    let inner = f.dir.join("in");
    std::fs::create_dir(&inner).unwrap();
    std::fs::write(f.dir.join("hide.css"), ".x{display:none}").unwrap();
    let page =
        r#"<link rel=stylesheet href="../hide.css"><p class=x>styled-away-text</p><p>kept</p>"#;
    std::fs::write(inner.join("p.html"), page).unwrap();
    let path = inner.join("p.html").display().to_string();
    f.config.allow = vec![inner.clone()];
    f.start();
    let mut m = f.mcp();
    m.ok("open", &path_arg(&path));
    assert!(
        m.ok("read", r#"{"page":"p1"}"#)
            .contains("styled-away-text")
    );

    let mut g = Fixture::new();
    g.config.allow = vec![f.dir.clone()];
    g.start();
    let mut m = g.mcp();
    m.ok("open", &path_arg(&path));
    assert!(
        !m.ok("read", r#"{"page":"p1"}"#)
            .contains("styled-away-text"),
        "control"
    );
}

#[test]
fn the_socket_is_private_and_never_replaces_another_file() {
    let f = Fixture::new();
    std::fs::write(&f.config.socket, "keep me").unwrap();
    assert!(Server::bind(f.config.clone()).is_err());
    assert_eq!(
        std::fs::read_to_string(&f.config.socket).unwrap(),
        "keep me"
    );
    std::fs::remove_file(&f.config.socket).unwrap();
    f.start();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&f.config.socket) & 0o077, 0, "no access for others");
    assert_eq!(mode(&f.config.audit), 0o600);

    let open = f.dir.join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
    let mut c = f.config.clone();
    c.socket = open.join("s");
    let e = Server::bind(c.clone()).err().expect("refused");
    assert!(e.to_string().contains("not sticky"), "{e}");
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o1777)).unwrap();
    c.audit = open.join("a.log");
    assert!(Server::bind(c).is_ok(), "a sticky directory will do");
}

#[test]
fn a_session_with_nothing_open_ends_after_the_idle_time() {
    let mut f = Fixture::new();
    f.config.idle = Duration::from_millis(200);
    f.start();
    let mut s = UnixStream::connect(&f.config.socket).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let t = Instant::now();
    assert_eq!(s.read(&mut [0u8; 8]).unwrap(), 0, "closed by the host");
    assert!(t.elapsed() < Duration::from_secs(4));
    assert!(f.audit().contains("\tidle\t"), "{}", f.audit());
}

#[test]
fn a_flood_of_requests_is_throttled_and_logged_in_few_lines() {
    let mut f = Fixture::new();
    f.config.rate = 1.0;
    f.config.burst = 3.0;
    f.start();
    let mut s = UnixStream::connect(&f.config.socket).unwrap();
    let mut refused = 0;
    for _ in 0..10 {
        let r = serve_fixture::exchange(&mut s, &Frame::new(kind::PAGES).encode_vec());
        refused += usize::from(r.get_num(tag::CODE) == Some(code::RATE_LIMITED));
    }
    assert!(refused >= 6, "{refused}");
    drop(s);
    std::thread::sleep(Duration::from_millis(200));
    let log = f.audit();
    let lines: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("\tthrottled\t"))
        .collect();
    assert!(lines.len() <= 2, "{log}");
    let total: usize = lines
        .iter()
        .map(|l| l.rsplit(" x").next().unwrap().parse::<usize>().unwrap())
        .sum();
    assert_eq!(total, refused, "{log}");
}

#[test]
fn the_host_wide_page_cap_holds_across_sessions() {
    let mut f = Fixture::new();
    f.config.max_total_pages = 2;
    f.start();
    let (mut a, mut b) = (f.mcp(), f.mcp());
    a.ok("open", &path_arg(&f.page()));
    b.ok("open", &path_arg(&f.page()));
    let (t, err) = b.call("open", &path_arg(&f.page()));
    assert!(err && t.contains("across its sessions"), "{t}");
    a.ok("close", r#"{"page":"p1"}"#);
    b.ok("open", &path_arg(&f.page()));
}

#[test]
fn a_file_name_cannot_break_a_line() {
    let f = Fixture::new();
    let odd = f.dir.join("a\nInjected line %.html");
    std::fs::write(&odd, "<h1>x</h1>").unwrap();
    f.start();
    let mut m = f.mcp();
    let opened = m.ok("open", &path_arg(&odd.display().to_string()));
    assert!(
        opened.contains("a%0AInjected%20line%20%25.html"),
        "{opened}"
    );
    assert!(
        !opened.lines().any(|l| l.starts_with("Injected")),
        "{opened}"
    );
    assert_eq!(m.ok("pages", "{}").lines().count(), 1);
}

#[test]
fn both_codecs_write_the_same_bytes_and_read_each_other() {
    use madcawl_mcp::frame as mf;
    assert_eq!(
        (mf::kind::OPEN, mf::kind::PAGES, mf::kind::ERROR),
        (kind::OPEN, kind::PAGES, kind::ERROR)
    );
    assert_eq!((mf::tag::PAGE, mf::tag::CHARSET), (tag::PAGE, tag::CHARSET));
    let a = Frame::new(kind::READ)
        .num(tag::PAGE, 3)
        .num(tag::BUDGET, 1 << 40)
        .str(tag::TEXT, "pub fn push 中")
        .str(tag::REF, "");
    let b = mf::Frame::new(mf::kind::READ)
        .num(mf::tag::PAGE, 3)
        .num(mf::tag::BUDGET, 1 << 40)
        .str(mf::tag::TEXT, "pub fn push 中")
        .str(mf::tag::REF, "");
    let bytes = a.encode_vec();
    assert_eq!(bytes, b.encode());
    let back = mf::Frame::decode_body(&bytes[4..]).unwrap();
    assert_eq!(back.fields, a.fields);
    let (again, used) = Frame::decode(&b.encode()).unwrap();
    assert_eq!((again, used), (a, bytes.len()));
}
