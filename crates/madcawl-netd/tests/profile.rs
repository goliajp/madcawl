//! A profile keeps the login across a restart of the network process,
//! survives a damaged store, and belongs to one process at a time. The
//! process is the binary, run as the product runs it: with a control
//! channel (`--control-fd`), the host logging in on its own connection and
//! the site's documents on connections bound to their registrations.

mod fixture;

use fixture::{LOGIN_BODY, exchange, get, hello, navigate, register, site, status_body, temp_path};
use madcawl_dom::{Dom, NodeId};
use madcawl_form::{Context, Files, State, submission};
use madcawl_html::{Options, Step, Tokenizer, TreeBuilder};
use madcawl_net::submit::{Navigation, fetch_frame};
use madcawl_net::wire::{Frame, kind, tag};
use madcawl_net::{Client, Config, Url};
use madcawl_netd::{Loaded, Profile, ProfileError};
use madcawl_str::atom;
use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

/// The network process on `profile`, and the host's end of its control
/// channel, which the child gets as descriptor 3.
fn netd(socket: &Path, profile: &Path) -> (Child, UnixStream) {
    let (host, theirs) = UnixStream::pair().unwrap();
    host.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let script = "exec \"$0\" --socket \"$1\" --profile \"$2\" --control-fd 3 3<&0 0</dev/null";
    let child = Command::new("/bin/sh")
        .args(["-c".as_ref(), script.as_ref(), env!("CARGO_BIN_EXE_madcawl-netd").as_ref(), socket.as_os_str(), profile.as_os_str()])
        .stdin(Stdio::from(OwnedFd::from(theirs)))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    (child, host)
}

/// What script reads of `document.cookie` for `url` on `s`.
fn script_cookies(s: &mut UnixStream, url: &str) -> Frame {
    Frame::new(kind::COOKIE_GET).num(tag::ID, 9).str(tag::URL, url).write_to(s).unwrap();
    Frame::read_from(s).unwrap().expect("a frame")
}

/// SIGTERM, then the exit status within ten seconds.
fn stop(child: Child) -> std::process::ExitStatus {
    signal(child, 15)
}

/// `sig`, then the exit status within ten seconds.
fn signal(mut child: Child, sig: i32) -> std::process::ExitStatus {
    assert_eq!(unsafe { kill(child.id() as i32, sig) }, 0);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            return st;
        }
        assert!(Instant::now() < deadline, "the network process did not stop");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn element(dom: &Dom, name: madcawl_str::Atom, nth: usize) -> NodeId {
    dom.descendants(dom.document()).filter(|&n| dom.local_name(n) == Some(name)).nth(nth).unwrap()
}

/// Logs in through the login page's form, as a person filling it in.
fn log_in(s: &mut UnixStream, origin: &str) -> (u64, String) {
    let page = Url::parse(&format!("{origin}/login")).unwrap();
    let (status, html) = status_body(&navigate(s, 1, page.as_str()));
    assert_eq!(status, 200);
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(Dom::new(), Options::default());
    tok.feed(html.as_bytes());
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    let dom = b.into_sink();
    let mut state = State::default();
    state.values.insert(element(&dom, atom!("input"), 0), "alice".into());
    state.values.insert(element(&dom, atom!("input"), 1), "s3cret".into());
    let no_files = |_: &Path| false;
    let cx = Context { document_url: &page, base_url: &page, files: Files { allow: &no_files, max_body: 1 << 20 } };
    let sub = submission(&dom, element(&dom, atom!("form"), 0), Some(element(&dom, atom!("button"), 0)), &state, &cx).unwrap();
    assert_eq!(sub.body.as_deref(), Some(LOGIN_BODY.as_bytes()));
    let nav = Navigation {
        id: 2,
        method: "POST",
        url: &sub.url,
        content_type: sub.content_type.as_deref(),
        body: sub.body.as_deref(),
        initiator: Some(&page),
    };
    status_body(&exchange(s, fetch_frame(&nav)))
}

#[test]
fn a_login_survives_a_restart_and_a_profile_has_one_owner() {
    let origin = site();
    let port = origin.rsplit(':').next().unwrap();
    let (sock, dir) = (temp_path("s"), temp_path("profile"));
    let (first, mut control) = netd(&sock, &dir);
    let mut host = hello(&sock, &register(&mut control, None));
    assert_eq!(log_in(&mut host, &origin), (200, "Welcome alice; cookies: sid=abc123; visit=1".into()));
    drop(host);
    assert!(stop(first).success());
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    assert!(dir.join("cookies.bin").is_file() && dir.join("downloads").is_dir());

    // the cookies the host's login navigation set reach the site's own documents after a restart
    let (second, mut control) = netd(&sock, &dir);
    let mut page = hello(&sock, &register(&mut control, Some(&format!("{origin}/login"))));
    let (status, body) = status_body(&get(&mut page, 3, &format!("{origin}/home")));
    assert_eq!((status, body.as_str()), (200, "Welcome alice; cookies: sid=abc123"), "the session cookie is gone, the login is not");
    let script = script_cookies(&mut page, &format!("{origin}/home"));
    assert_eq!((script.kind, script.get_str(tag::TEXT)), (kind::COOKIES, Some("")), "the login cookie is HttpOnly");
    // another site's document gets the page without them (Lax), and may not claim the site's initiator
    let mut other = hello(&sock, &register(&mut control, Some(&format!("http://localhost:{port}/"))));
    assert_eq!(status_body(&get(&mut other, 4, &format!("{origin}/home"))).0, 403);
    let forged = Frame::new(kind::FETCH).num(tag::ID, 5).str(tag::URL, &format!("{origin}/home")).str(tag::INITIATOR, &origin);
    assert_eq!(exchange(&mut other, forged).last().unwrap().get_num(tag::REFUSED), Some(1));

    let (sock2, other_dir) = (temp_path("s"), temp_path("profile"));
    let (fresh, mut fresh_control) = netd(&sock2, &other_dir);
    let mut fresh_host = hello(&sock2, &register(&mut fresh_control, None));
    assert_eq!(status_body(&navigate(&mut fresh_host, 6, &format!("{origin}/home"))).0, 403, "a profile without the login is turned away");
    drop(fresh_host);
    assert!(stop(fresh).success());
    std::fs::remove_dir_all(&other_dir).unwrap();

    let third = netd(&temp_path("s"), &dir).0.wait_with_output().unwrap();
    assert!(!third.status.success());
    let err = String::from_utf8_lossy(&third.stderr);
    assert!(err.contains("is in use by another network process"), "{err}");
    drop((page, other));
    assert!(stop(second).success());
    std::fs::remove_dir_all(&dir).unwrap();
}

fn client() -> Arc<Client> {
    Arc::new(Client::new(Config::default()))
}

fn set(c: &Client, line: &str) {
    let u = Url::parse("https://example.com/").unwrap();
    assert!(c.lock_jar().set(&u, line, madcawl_net::cookie::Api::Http, madcawl_net::fetch::now_secs()));
}

#[test]
fn a_damaged_store_is_set_aside_and_the_jar_starts_empty() {
    let dir = temp_path("profile");
    let c = client();
    let p = Profile::open(&dir, c.clone()).unwrap();
    assert_eq!(p.loaded(), Loaded::Fresh);
    set(&c, "a=1; Max-Age=600");
    set(&c, "b=2; Max-Age=600");
    drop(p);
    let good = std::fs::read(dir.join("cookies.bin")).unwrap();
    assert_eq!(Profile::open(&dir, client()).unwrap().loaded(), Loaded::Cookies(2));

    std::fs::write(dir.join("cookies.bin"), &good[..good.len() - 5]).unwrap();
    let c = client();
    let p = Profile::open(&dir, c.clone()).unwrap();
    assert_eq!(p.loaded(), Loaded::Corrupt(madcawl_net::cookie::StoreError::Truncated));
    assert!(c.lock_jar().is_empty());
    assert!(!dir.join("cookies.bin").exists());
    assert_eq!(std::fs::read(dir.join("cookies.bin.corrupt")).unwrap(), good[..good.len() - 5]);
    drop(p);

    let mut flipped = good.clone();
    *flipped.last_mut().unwrap() ^= 0x40;
    std::fs::write(dir.join("cookies.bin"), &flipped).unwrap();
    let p = Profile::open(&dir, client()).unwrap();
    assert_eq!(p.loaded(), Loaded::Corrupt(madcawl_net::cookie::StoreError::Checksum));
    drop(p);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_second_open_of_a_held_profile_is_refused() {
    let dir = temp_path("profile");
    let p = Profile::open(&dir, client()).unwrap();
    assert!(matches!(Profile::open(&dir, client()), Err(ProfileError::InUse(_))));
    drop(p);
    assert!(Profile::open(&dir, client()).is_ok(), "closing lets go of the lock");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn changes_are_saved_soon_and_only_when_a_persistent_cookie_changed() {
    let dir = temp_path("profile");
    let c = client();
    let p = Profile::open(&dir, c.clone()).unwrap();
    let store = dir.join("cookies.bin");
    set(&c, "session=1");
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!store.exists(), "a session cookie is not saved");
    set(&c, "kept=1; Max-Age=600");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !store.exists() {
        assert!(Instant::now() < deadline, "the store was not written");
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut jar = madcawl_net::cookie::CookieJar::new(Default::default());
    assert_eq!(jar.load_persistent(&std::fs::read(&store).unwrap(), madcawl_net::fetch::now_secs()), Ok(1));
    drop(p);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_hangup_saves_the_cookies_changed_in_the_last_second() {
    let origin = site();
    let (sock, dir) = (temp_path("s"), temp_path("profile"));
    let (netd, mut control) = netd(&sock, &dir);
    let mut host = hello(&sock, &register(&mut control, None));
    assert_eq!(log_in(&mut host, &origin).0, 200);
    drop(host);
    // well inside the writer's one-second wait
    assert!(signal(netd, 1).success());
    let mut jar = madcawl_net::cookie::CookieJar::new(Default::default());
    assert_eq!(jar.load_persistent(&std::fs::read(dir.join("cookies.bin")).unwrap(), madcawl_net::fetch::now_secs()), Ok(1));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn only_partial_downloads_are_removed_at_open() {
    let dir = temp_path("profile");
    drop(Profile::open(&dir, client()).unwrap());
    let downloads = dir.join("downloads");
    for n in ["movie.part", ".movie.part", ".x.part", "notes.txt"] {
        std::fs::write(downloads.join(n), b"x").unwrap();
    }
    drop(Profile::open(&dir, client()).unwrap());
    let mut left: Vec<String> = std::fs::read_dir(&downloads).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    left.sort();
    assert_eq!(left, ["movie.part", "notes.txt"], "a finished download named .part is kept");
    std::fs::remove_dir_all(&dir).unwrap();
}
