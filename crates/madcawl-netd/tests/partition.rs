//! Agent sessions on one network process, each in a cookie partition of
//! its own: a login in one is not seen by another nor by the profile,
//! lasts as long as its partition, and goes when the host drops it; a
//! content process cannot name another partition; partitions and their
//! jars are capped; and a profile never saves or lends a partition's
//! cookies.

mod fixture;

use fixture::{LOGIN_BODY, assert_refused, connect, exchange, hello, site, status_body, temp_path};
use madcawl_net::cookie::JarLimits;
use madcawl_net::wire::{Frame, kind, tag};
use madcawl_net::{Client, Config};
use madcawl_netd::{Loaded, Profile, ServerLimits, serve_guarded, serve_streams};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct Netd {
    path: PathBuf,
    control: UnixStream,
    client: Arc<Client>,
}

impl Drop for Netd {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn netd(config: Config) -> Netd {
    let path = temp_path("p");
    let l = UnixListener::bind(&path).unwrap();
    let (host, theirs) = UnixStream::pair().unwrap();
    host.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let client = Arc::new(Client::new(config));
    let c = client.clone();
    std::thread::spawn(move || serve_guarded(l, theirs, c, ServerLimits::default(), None).unwrap());
    Netd { path, control: host, client }
}

impl Netd {
    fn control(&mut self, f: Frame) -> Frame {
        f.write_to(&mut self.control).unwrap();
        Frame::read_from(&mut self.control).unwrap().expect("a frame")
    }

    /// Registers the host's own connection (`doc` `None`) or a document,
    /// in `partition` or, without one, the profile's jar.
    fn register(&mut self, doc: Option<&str>, partition: Option<u64>) -> Frame {
        let f = Frame::new(kind::REGISTER).num(tag::ID, 1);
        let f = match doc {
            Some(u) => f.str(tag::URL, u),
            None => f.num(tag::NAVIGATION, 1),
        };
        self.control(match partition {
            Some(p) => f.num(tag::PARTITION, p),
            None => f,
        })
    }

    fn connect(&mut self, doc: Option<&str>, partition: Option<u64>) -> UnixStream {
        let r = self.register(doc, partition);
        assert_eq!(r.kind, kind::REGISTERED, "{r:?}");
        hello(&self.path, r.field(tag::TOKEN).unwrap())
    }

    /// Drops `partition`: whether it was open, and the registrations
    /// dropped with it.
    fn drop_partition(&mut self, partition: u64) -> (u64, u64) {
        let r = self.control(Frame::new(kind::PARTITION_DROP).num(tag::ID, 3).num(tag::PARTITION, partition));
        assert_eq!(r.kind, kind::PARTITION_DROP, "{r:?}");
        (r.get_num(tag::OK).unwrap(), r.get_num(tag::COUNT).unwrap())
    }
}

/// Posts the login form as a navigation; the status and body it ends at.
fn log_in(s: &mut UnixStream, origin: &str) -> (u64, String) {
    let f = Frame::new(kind::FETCH)
        .num(tag::ID, 1)
        .str(tag::URL, &format!("{origin}/session"))
        .str(tag::METHOD, "POST")
        .num(tag::NAVIGATION, 1)
        .header("Content-Type", b"application/x-www-form-urlencoded")
        .bytes(tag::BODY, LOGIN_BODY.as_bytes());
    status_body(&exchange(s, f))
}

/// The status of `/home`, which only a request carrying the login's
/// `sid` cookie gets 200 for.
fn home(s: &mut UnixStream, origin: &str) -> u64 {
    let f = Frame::new(kind::FETCH).num(tag::ID, 2).str(tag::URL, &format!("{origin}/home")).num(tag::NAVIGATION, 1);
    status_body(&exchange(s, f)).0
}

/// The one frame a cookie call is answered with.
fn one(s: &mut UnixStream, f: Frame) -> Frame {
    f.write_to(s).unwrap();
    Frame::read_from(s).unwrap().expect("a frame")
}

fn script_cookies(s: &mut UnixStream, url: &str, partition: Option<u64>) -> Frame {
    let f = Frame::new(kind::COOKIE_GET).num(tag::ID, 9).str(tag::URL, url);
    one(s, partition.map_or(f.clone(), |p| f.num(tag::PARTITION, p)))
}

#[test]
fn a_login_stays_in_its_sessions_partition_until_the_host_drops_it() {
    let origin = site();
    let mut n = netd(Config::default());
    let (mut a, mut b) = (n.connect(None, Some(1)), n.connect(None, Some(2)));
    let (status, body) = log_in(&mut a, &origin);
    assert!(status == 200 && body.starts_with("Welcome alice"), "{status} {body}");
    assert_eq!(home(&mut b, &origin), 403, "session B does not see A's login");
    let mut profile = n.connect(None, None);
    assert_eq!(home(&mut profile, &origin), 403, "nor does the profile's jar");
    assert!(n.client.lock_jar().is_empty());
    // a second page of A, on a connection of its own, is still logged in
    let mut a2 = n.connect(None, Some(1));
    assert_eq!(home(&mut a2, &origin), 200);
    assert_eq!(n.client.jar_of(1).unwrap().lock().unwrap().len(), 2, "sid and visit");

    assert_eq!(n.drop_partition(1), (1, 2), "A's partition and its two registrations");
    assert!(n.client.jar_of(1).is_none() && n.client.partitions().len() == 1);
    let f = Frame::new(kind::FETCH).num(tag::ID, 5).str(tag::URL, &format!("{origin}/home"));
    assert_refused(&exchange(&mut a, f));
    assert_eq!(n.drop_partition(1), (0, 0), "dropped already");
    // the session number used again starts with no cookies
    let mut again = n.connect(None, Some(1));
    assert_eq!(home(&mut again, &origin), 403);
    assert_eq!(home(&mut b, &origin), 403);
}

#[test]
fn a_content_process_cannot_name_another_partition() {
    let origin = site();
    let mut n = netd(Config::default());
    let mut a = n.connect(None, Some(1));
    log_in(&mut a, &origin);
    let doc = format!("{origin}/home");
    let mut mine = n.connect(Some(&doc), Some(1));
    let mut other = n.connect(Some(&doc), Some(2));
    // script in A's document sees A's script-visible cookie, never the HttpOnly one
    let r = script_cookies(&mut mine, &doc, None);
    assert_eq!((r.kind, r.get_str(tag::TEXT)), (kind::COOKIES, Some("visit=1")), "{r:?}");
    assert_eq!(script_cookies(&mut mine, &doc, Some(1)).get_str(tag::TEXT), Some("visit=1"));
    // B's document asking for A's partition, or the profile's, is refused
    for p in [1, 0] {
        assert_refused(&[script_cookies(&mut other, &doc, Some(p))]);
        let f = Frame::new(kind::FETCH).num(tag::ID, 6).str(tag::URL, &doc).num(tag::NAVIGATION, 0).num(tag::PARTITION, p);
        assert_refused(&exchange(&mut other, f));
    }
    let set = Frame::new(kind::COOKIE_SET).num(tag::ID, 7).str(tag::URL, &doc).str(tag::TEXT, "sid=forged").num(tag::PARTITION, 1);
    assert_refused(&[one(&mut other, set)]);
    assert_eq!(script_cookies(&mut other, &doc, None).get_str(tag::TEXT), Some(""));
    assert_eq!(home(&mut a, &origin), 200, "A's login is untouched");
    // the host's own connection is held to its registration's partition too
    let f = Frame::new(kind::FETCH).num(tag::ID, 8).str(tag::URL, &doc).num(tag::NAVIGATION, 1).num(tag::PARTITION, 2);
    assert_refused(&exchange(&mut a, f));
    // a registration cannot be moved into another partition
    let token = n.register(Some(&doc), Some(2)).field(tag::TOKEN).unwrap().to_vec();
    let moved = n.control(Frame::new(kind::REGISTER).num(tag::ID, 1).str(tag::URL, &doc).bytes(tag::TOKEN, &token).num(tag::PARTITION, 1));
    assert_eq!(moved.kind, kind::ERROR, "{moved:?}");
}

#[test]
fn partitions_and_their_jars_are_capped() {
    let config = Config { partitions: 2, partition_jar: JarLimits { per_domain: 10, total: 4 }, ..Config::default() };
    let mut n = netd(config);
    let doc = "http://127.0.0.1/";
    let mut a = n.connect(Some(doc), Some(1));
    n.connect(None, Some(2));
    let full = n.register(None, Some(3));
    assert_eq!((full.kind, full.get_str(tag::MESSAGE)), (kind::ERROR, Some("too many cookie partitions: at most 2")));
    assert_eq!(n.register(None, Some(1)).kind, kind::REGISTERED, "an open partition takes more registrations");
    let reserved = n.control(Frame::new(kind::PARTITION_DROP).num(tag::ID, 3).num(tag::PARTITION, 0));
    assert_eq!(reserved.kind, kind::ERROR, "the profile's jar is never dropped");
    for i in 0..10 {
        let set = Frame::new(kind::COOKIE_SET).num(tag::ID, i).str(tag::URL, doc).str(tag::TEXT, &format!("c{i}=v"));
        assert_eq!(one(&mut a, set).get_num(tag::OK), Some(1));
    }
    assert_eq!(n.client.jar_of(1).unwrap().lock().unwrap().len(), 4, "the jar's total");
    n.drop_partition(2);
    assert_eq!(n.register(None, Some(3)).kind, kind::REGISTERED, "a dropped partition makes room");
}

#[test]
fn a_profile_never_lends_or_saves_a_partitions_cookies() {
    let origin = site();
    let dir = temp_path("prof");
    let mut n = netd(Config::default());
    let profile = Profile::open(&dir, n.client.clone()).unwrap();
    let mut user = n.connect(None, None);
    log_in(&mut user, &origin);
    assert_eq!(home(&mut user, &origin), 200, "the user's login, in the profile");
    let mut agent = n.connect(None, Some(7));
    assert_eq!(home(&mut agent, &origin), 403, "an agent session's page has none of the user's login");
    let doc = format!("{origin}/home");
    let mut page = n.connect(Some(&doc), Some(7));
    assert_eq!(script_cookies(&mut page, &doc, None).get_str(tag::TEXT), Some(""));
    let set = Frame::new(kind::COOKIE_SET).num(tag::ID, 1).str(tag::URL, &doc).str(tag::TEXT, "agent=1; Max-Age=3600");
    assert_eq!(one(&mut page, set).get_num(tag::OK), Some(1));
    drop(profile);
    // only the user's persistent cookie was saved
    let fresh = Arc::new(Client::new(Config::default()));
    let reopened = Profile::open(&dir, fresh.clone()).unwrap();
    assert_eq!(reopened.loaded(), Loaded::Cookies(1));
    let saved = fresh.lock_jar().header(&madcawl_net::Url::parse(&doc).unwrap(), madcawl_net::cookie::SiteContext::SameSite, 0);
    assert_eq!(saved.as_deref(), Some("sid=abc123"));
    drop(reopened);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_partition_the_host_does_not_spell_out_is_refused() {
    let mut n = netd(Config::default());
    let reg = || Frame::new(kind::REGISTER).num(tag::ID, 4).num(tag::NAVIGATION, 1);
    for bad in [reg().bytes(tag::PARTITION, &[0x80]), reg().num(tag::PARTITION, 1).num(tag::PARTITION, 2)] {
        let r = n.control(bad);
        assert_eq!((r.kind, r.get_str(tag::MESSAGE)), (kind::ERROR, Some("bad partition")), "{r:?}");
    }
    assert!(n.client.partitions().is_empty(), "nothing was opened, not even the profile's by mistake");
}

#[test]
fn without_a_guard_every_frame_is_the_profiles() {
    let path = temp_path("u");
    let l = UnixListener::bind(&path).unwrap();
    let client = Arc::new(Client::new(Config::default()));
    let c = client.clone();
    std::thread::spawn(move || serve_streams(l.incoming(), c, ServerLimits::default()));
    let mut s = connect(&path);
    let doc = "http://127.0.0.1/";
    let set = Frame::new(kind::COOKIE_SET).num(tag::ID, 1).str(tag::URL, doc).str(tag::TEXT, "a=1").num(tag::PARTITION, 5);
    assert_eq!(one(&mut s, set).get_num(tag::OK), Some(1), "stored, in the profile's jar");
    assert_eq!(script_cookies(&mut s, doc, Some(9)).get_str(tag::TEXT), Some("a=1"));
    assert_eq!(client.lock_jar().len(), 1);
    assert!(client.partitions().is_empty());
    let _ = std::fs::remove_file(&path);
}
