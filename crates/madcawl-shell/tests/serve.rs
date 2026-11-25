//! The host over its socket: through madcawl-mcp's JSON-RPC side as an
//! agent sees it, and with raw frames for what a client must not be
//! able to do.

mod serve_fixture;

use std::time::Duration;

use madcawl_agent::{Frame, kind, tag};
use madcawl_shell::serve::code;
use serve_fixture::{EncodeVec as _, Fixture, path_arg, raw};

#[test]
fn an_agent_reads_a_page_over_the_socket() {
    let f = Fixture::new();
    f.start();
    let mut m = f.mcp();
    let opened = m.ok("open", &path_arg(&f.page()));
    assert!(opened.starts_with("opened p1 file:///"), "{opened}");
    assert!(opened.contains("Vec in std"), "{opened}");
    let read = m.ok("read", r#"{"page":"p1"}"#);
    assert!(read.contains("pub fn push(&mut self, value: T)"), "{read}");
    assert!(!read.contains("hidden instructions"), "{read}");
    let hits = m.ok("find", r#"{"page":"p1","role":"link"}"#);
    assert!(hits.contains("Other page"), "{hits}");
    let seq: u64 = read
        .split(" · seq ")
        .nth(1)
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let diff = m.ok("diff", &format!(r#"{{"page":"p1","since":{seq}}}"#));
    assert!(diff.contains("<<page-content-"), "{diff}");
    assert!(m.ok("pages", "{}").contains("p1 file:///"));
    assert!(m.ok("close", r#"{"page":"p1"}"#).contains("closed p1"));
    let (gone, err) = m.call("snapshot", r#"{"page":"p1"}"#);
    assert!(err && gone.contains("no such page"), "{gone}");
}

#[test]
fn page_content_comes_in_an_envelope_that_the_page_cannot_close() {
    let f = Fixture::new();
    f.start();
    let mut m = f.mcp();
    m.ok("open", &path_arg(&f.page()));
    let snap = m.ok("snapshot", r#"{"page":"p1"}"#);
    let body = snap.split_once('\n').unwrap().1;
    assert!(
        body.starts_with("The text between the <<page-content-"),
        "{snap}"
    );
    let marker = body
        .split("<<")
        .nth(1)
        .unwrap()
        .split(">>")
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(snap.matches(&format!("<</{marker}>>")).count(), 1, "{snap}");
    assert!(
        snap.contains("<\\</page-content-0000000000000000>>"),
        "{snap}"
    );
    let again = m.ok("snapshot", r#"{"page":"p1"}"#);
    assert!(!again.contains(&marker), "a fresh marker each reply");
}

#[test]
fn only_local_files_under_the_allowed_directories_open() {
    let f = Fixture::new();
    f.start();
    let mut m = f.mcp();
    for url in [
        "https://example.com/",
        "http://localhost/x.html",
        "data:text/html,<p>x",
    ] {
        let (t, err) = m.call("open", &path_arg(url));
        assert!(err && t.contains("only local files"), "{url}: {t}");
    }
    let (t, err) = m.call("open", &path_arg("/etc/hosts"));
    assert!(err && t.contains("outside the directories"), "{t}");
    let url = format!("file://{}", f.page());
    assert!(m.ok("open", &path_arg(&url)).starts_with("opened p1"));
    assert!(
        m.ok("open", &path_arg("page.html"))
            .starts_with("opened p2"),
        "relative to the cwd"
    );
    assert!(
        f.audit().contains("\topen\t-\t-\terror 11\n"),
        "{}",
        f.audit()
    );
}

#[test]
fn the_page_cap_and_the_session_cap_hold() {
    let mut f = Fixture::new();
    f.config.max_pages = 2;
    f.config.max_sessions = 1;
    f.start();
    let mut m = f.mcp();
    m.ok("open", &path_arg(&f.page()));
    m.ok("open", &path_arg(&f.page()));
    let (t, err) = m.call("open", &path_arg(&f.page()));
    assert!(err && t.contains("2 pages open already"), "{t}");
    m.ok("close", r#"{"page":"p1"}"#);
    m.ok("open", &path_arg(&f.page()));
    let reply = raw(&f, &Frame::new(kind::PAGES).encode_vec());
    assert_eq!(reply.get_num(tag::CODE), Some(code::TOO_MANY_SESSIONS));
}

#[test]
fn bad_frames_end_the_session_with_an_error() {
    let f = Fixture::new();
    f.start();
    let too_long = ((f.config.max_request + 1) as u32).to_le_bytes();
    assert_eq!(
        raw(&f, &too_long).get_num(tag::CODE),
        Some(code::REQUEST_TOO_LARGE)
    );
    let malformed = [5, 0, 0, 0, 10, 0, 3, 9, 0];
    assert_eq!(
        raw(&f, &malformed).get_num(tag::CODE),
        Some(code::BAD_REQUEST)
    );
    let unknown = Frame::new(999).encode_vec();
    assert_eq!(raw(&f, &unknown).get_num(tag::CODE), Some(code::UNKNOWN));
    let no_page = Frame::new(kind::READ).num(tag::PAGE, 9).encode_vec();
    assert_eq!(raw(&f, &no_page).get_num(tag::CODE), Some(code::NO_PAGE));
}

#[test]
fn the_audit_log_has_a_line_a_request_and_no_page_text() {
    let mut f = Fixture::new();
    f.config.idle = Duration::from_millis(200);
    f.start();
    let mut m = f.mcp();
    m.ok("open", &path_arg(&f.page()));
    let snap = m.ok("snapshot", r#"{"page":"p1"}"#);
    let line = snap.lines().find(|l| l.contains("heading")).unwrap();
    let r = &line[line.find("[e").unwrap() + 1..line.rfind(']').unwrap()];
    m.ok("read", &format!(r#"{{"page":"p1","ref":"{r}"}}"#));
    m.ok("read", r#"{"page":"p1"}"#);
    std::thread::sleep(Duration::from_millis(300));
    // the page has expired; the session, left with none, may have ended too
    let (t, err) = m.call("outline", r#"{"page":"p1"}"#);
    assert!(
        err && (t.contains("no such page") || t.contains("broke")),
        "{t}"
    );
    let log = f.audit();
    let lines: Vec<&str> = log.lines().collect();
    assert!(lines.len() >= 5, "{log}");
    for l in &lines {
        assert_eq!(l.split('\t').count(), 7, "{l}");
    }
    let actions: Vec<&str> = lines
        .iter()
        .map(|l| l.split('\t').nth(3).unwrap())
        .collect();
    assert_eq!(
        actions[..5],
        ["open", "snapshot", "markdown", "markdown", "expire"],
        "{log}"
    );
    assert!(lines[2].contains("\theading\tVec in std\tok"), "{log}");
    assert!(
        !log.contains("zebra-secret-paragraph") && !log.contains("push(&mut"),
        "{log}"
    );
}
