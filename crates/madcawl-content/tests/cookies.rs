//! `document.cookie` of documents loaded over the network: the
//! session's jar in the network service, in the host's own process and in
//! a content process. The site is a small server on 127.0.0.1.

mod common;

use std::net::TcpStream;
use std::time::Duration;

use common::{Host, JS, executable, open_at, respond, site};
use madcawl_mcp::json;

/// A page whose script reads and writes its cookies, then loads a script
/// that shows the cookies its request carried.
const JAR: &str = r#"<!doctype html><title>Jar</title><p id=out></p><script>
function log(s) { document.getElementById('out').appendChild(document.createTextNode('|' + s)); }
log('seen:' + document.cookie);
document.cookie = 'js=j1; path=/'; document.cookie = 'hid=forged; path=/';
log('now:' + document.cookie);
</script><script src=/sent.js></script>"#;

/// A page whose script writes a cookie named for its query and then
/// waits on a script that never comes.
const STUCK: &str = r#"<!doctype html><title>Stuck</title><script>
document.cookie = 'p' + location.search.slice(1) + '=1; path=/'; document.cookie;
</script><script src=/hang.js></script>"#;

const SHOW: &str = r#"<!doctype html><title>Show</title><p id=out></p><script>
document.getElementById('out').textContent = 'c:' + document.cookie;
</script>"#;

fn route(s: &mut TcpStream, path: &str, cookie: &str) -> bool {
    match path {
        "/" => respond(s, "200 OK", "Content-Type: text/html\r\n", b"<p>blank</p>"),
        "/jar" => respond(s, "200 OK", "Content-Type: text/html\r\nSet-Cookie: hid=h1; HttpOnly; Path=/\r\nSet-Cookie: vis=v1; Path=/\r\n", JAR.as_bytes()),
        "/sent.js" => respond(s, "200 OK", JS, format!("log('sent:{}')", cookie.trim()).as_bytes()),
        "/show" => respond(s, "200 OK", "Content-Type: text/html\r\n", SHOW.as_bytes()),
        p if p.starts_with("/stuck?") => respond(s, "200 OK", "Content-Type: text/html\r\n", STUCK.as_bytes()),
        "/hang.js" => std::thread::sleep(Duration::from_secs(30)),
        _ => return false,
    }
    true
}

/// `document.cookie` reads and writes the session's jar in the network
/// service, without its `HttpOnly` cookies, and what script set goes with
/// the page's next request: in the host's own process and in a content
/// process.
#[test]
fn document_cookie_is_the_session_s_jar_in_the_network_service() {
    let origin = site(route);
    let want = "|seen:vis=v1|now:vis=v1; js=j1|sent:hid=h1; vis=v1; js=j1";
    let mut h = Host::start();
    let opened = h.call("open", &format!("{{\"path\":{}}}", json::s(&format!("{origin}/jar")).to_json()));
    let page = opened.split_whitespace().nth(1).unwrap().to_owned();
    let text = h.call("read", &format!(r#"{{"page":"{page}"}}"#));
    assert!(text.contains(want), "{text}");
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mcc-jar-{}", std::process::id()));
    let (_child, mut h) = executable(&dir, &[]);
    let (opened, _) = open_at(&mut h, &format!("{origin}/jar"));
    let page = opened.split_whitespace().nth(1).unwrap().to_owned();
    let text = h.call("read", &format!(r#"{{"page":"{page}"}}"#));
    assert!(text.contains(want), "in a content process: {text}");
}

/// Pages left waiting on a script after theirs touched `document.cookie`
/// hold one network registration each, not two: the cookie connection
/// goes when a turn of script ends, so a session's share of
/// registrations still covers its pages and their navigations.
#[test]
fn pages_waiting_after_their_script_used_cookies_hold_no_cookie_connection() {
    let origin = site(route);
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mcc-turns-{}", std::process::id()));
    let (_child, mut h) = executable(&dir, &[]);
    for n in 0..5 {
        let (opened, error) = open_at(&mut h, &format!("{origin}/"));
        assert!(!error, "page {n}: {opened}");
        let page = opened.split_whitespace().nth(1).unwrap().to_owned();
        let to = json::s(&format!("{origin}/stuck?{n}")).to_json();
        let acted = h.call("act", &format!(r#"{{"page":"{page}","action":"navigate","value":{to},"timeout_ms":800}}"#));
        assert!(acted.contains("its window has not loaded"), "page {n}: {acted}");
    }
    let (opened, error) = open_at(&mut h, &format!("{origin}/show"));
    assert!(!error, "a sixth page: {opened}");
    let page = opened.split_whitespace().nth(1).unwrap().to_owned();
    let text = h.call("read", &format!(r#"{{"page":"{page}"}}"#));
    assert!(text.contains("p0=1; p1=1; p2=1; p3=1; p4=1"), "every waiting page wrote its cookie: {text}");
}
