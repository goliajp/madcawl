//! Each agent session's web pages keep their cookies in a jar of the
//! session's own: a login in one session is not seen by another, lasts
//! across the session's pages, and is dropped when the session ends, so
//! that the jars the host keeps never outnumber its sessions.

mod act_site;

use act_site::{Fixture, Mcp, act, page_of, path_arg, ref_on, site};
use std::time::{Duration, Instant};

/// A host with the network on, at most two sessions at a time and
/// password forms approved for `origin`; started.
fn host(origin: &str) -> Fixture {
    let mut f = Fixture::new();
    f.config.net = true;
    f.config.downloads = Some(f.dir.join("downloads"));
    f.config.approve = vec![origin.to_owned()];
    f.config.max_sessions = 2;
    f.start();
    f
}

fn log_in(m: &mut Mcp, origin: &str) {
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/login"))));
    let snap = m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#));
    let (user, pass) = (ref_on(&snap, "User"), ref_on(&snap, "Password"));
    let fields = format!(r#""fields":{{"{user}":"alice","{pass}":"s3cret"}},"submit":true"#);
    let (t, err) = act(m, &p, &format!(r#""action":"fill_form",{fields}"#));
    assert!(!err && t.contains("Welcome alice"), "{t}");
}

/// What a new page on `/home` says: a welcome only with the login's cookie.
fn home(m: &mut Mcp, origin: &str) -> String {
    let opened = m.ok("open", &path_arg(&format!("{origin}/home")));
    read(m, &opened)
}

fn read(m: &mut Mcp, opened: &str) -> String {
    m.ok("read", &format!(r#"{{"page":"{}"}}"#, page_of(opened)))
}

#[test]
fn each_session_has_cookies_of_its_own_which_end_with_it() {
    let origin = site();
    let f = host(&origin);
    let (mut a, mut b) = (f.mcp(), f.mcp());
    log_in(&mut a, &origin);
    let other = home(&mut b, &origin);
    assert!(other.contains("Log in first") && !other.contains("Welcome"), "{other}");
    let again = home(&mut a, &origin);
    assert!(again.contains("Welcome alice"), "a second page of the session is logged in: {again}");
    drop(a);
    // both of the host's two jars are taken until A's is dropped, so a
    // third session's web page opens only once it has been
    let mut c = f.mcp();
    let deadline = Instant::now() + Duration::from_secs(10);
    let opened = loop {
        let (t, err) = c.call("open", &path_arg(&format!("{origin}/home")));
        if !err {
            break read(&mut c, &t);
        }
        assert!(Instant::now() < deadline, "the third session never got a jar: {t}");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(opened.contains("Log in first"), "{opened}");
    let still = home(&mut b, &origin);
    assert!(still.contains("Log in first"), "{still}");
}
