//! The executable's limit on how long a page's script runs at a stretch
//! (`--script-ms`): a script past it is stopped in its content process,
//! which goes on; a content process stuck outside script is still the
//! hang watchdog's (`--hang-secs`).

mod common;

use std::net::TcpStream;
use std::time::{Duration, Instant};

use common::{executable, open_at, respond};

const SPIN: &str = "<!doctype html><p id=out>before</p><script>for (;;) {}</script>\
<script>document.getElementById('out').textContent = 'after';</script>";

fn route(s: &mut TcpStream, path: &str, _: &str) -> bool {
    match path {
        "/" => respond(s, "200 OK", "Content-Type: text/html\r\n", b"<p>plain</p>"),
        "/spin" => respond(s, "200 OK", "Content-Type: text/html\r\n", SPIN.as_bytes()),
        _ => return false,
    }
    true
}

fn page_of(opened: &str) -> String {
    opened.split_whitespace().nth(1).unwrap().to_owned()
}

#[test]
fn a_script_past_the_limit_is_stopped_and_its_content_process_goes_on() {
    let origin = common::site(route);
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mcc-limit-{}", std::process::id()));
    let (child, mut h) = executable(&dir, &["--script-ms", "300", "--hang-secs", "30"]);
    let (opened, error) = open_at(&mut h, &format!("{origin}/"));
    assert!(!error, "{opened}");
    let content = child.content_pids();
    assert_eq!(content.lines().count(), 1, "{content}");
    let t0 = Instant::now();
    let (opened, error) = open_at(&mut h, &format!("{origin}/spin"));
    assert!(!error, "{opened}");
    let page = page_of(&opened);
    let text = h.call("read", &format!(r#"{{"page":"{page}"}}"#));
    assert!(text.contains("after"), "{text}");
    let log = h.call("requests", &format!(r#"{{"page":"{page}"}}"#));
    let stops = log.matches("script: uncaught exception: script stopped: it ran past the time limit").count();
    assert_eq!(stops, 1, "{log}");
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
    assert_eq!(child.content_pids(), content, "the same content process");
}

/// SIGSTOP holds the content process outside any script.
#[test]
fn a_content_process_stuck_outside_script_is_still_ended_by_the_hang_watchdog() {
    let origin = common::site(route);
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mcc-stuck-{}", std::process::id()));
    let (child, mut h) = executable(&dir, &["--script-ms", "300", "--hang-secs", "2"]);
    let (opened, error) = open_at(&mut h, &format!("{origin}/"));
    assert!(!error, "{opened}");
    let content = child.content_pids();
    let pid = content.trim();
    assert!(std::process::Command::new("kill").args(["-STOP", pid]).status().unwrap().success());
    let (text, error) = open_at(&mut h, &format!("{origin}/"));
    assert!(error && text.contains("did not answer within 2 s"), "{text}");
    let (opened, error) = open_at(&mut h, &format!("{origin}/"));
    assert!(!error, "{opened}");
    let now = child.content_pids();
    assert!(!now.is_empty() && now != content, "a new content process: {content} then {now}");
}
