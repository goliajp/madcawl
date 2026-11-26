use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use madcawl_agent::{Frame, kind, tag};
use madcawl_net::wire as netw;

use super::Proxy;
use super::content::Content;
use crate::serve::conn::PageState;
use crate::serve::link::{internal, read_until, write};
use crate::serve::net::partition_of;
use crate::serve::{AuditLog, Config, Net, Shared, code};

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

/// A test's directory, removed with it.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A host's shared state, with a network service on a thread when `net`.
fn shared(name: &str, net: bool, with: impl FnOnce(&mut Config)) -> (Shared, Dir) {
    let d = std::env::temp_dir().join(format!("mc-proxy-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let mut c = Config::new(d.join("s"));
    with(&mut c);
    let log = AuditLog::open(&d.join("audit.log")).unwrap();
    let net = net.then(|| Arc::new(Net::start(&d.join("s.net"), None, 8).unwrap()));
    (Shared::new(c, log, net, "run".into(), None), Dir(d))
}

fn page(id: u32) -> PageState {
    PageState { id, width: 800, height: 600, y: 0, url: "file:///a.html".into() }
}

fn code_of(f: &Frame) -> Option<u64> {
    (f.kind == kind::ERROR).then(|| f.get_num(tag::CODE)).flatten()
}

#[test]
fn a_content_process_gets_registrations_only_in_its_own_sessions_partition() {
    let (s, _dir) = shared("partition", true, |_| {});
    let net = s.net.clone().unwrap();
    let (mut one, mut two) = (Proxy::new(1, &s), Proxy::new(2, &s));
    let theirs = two.answer(&Frame::new(internal::NET_REGISTER)).unwrap();
    let theirs = theirs.field(tag::DATA).unwrap().to_vec();
    // session 1's process names session 2's partition, then the profile's,
    // then asks for session 2's partition to be dropped in the old form
    let drop_two = netw::Frame::new(netw::kind::PARTITION_DROP).num(netw::tag::PARTITION, partition_of(2));
    let mut encoded = Vec::new();
    drop_two.encode(&mut encoded);
    for q in [
        Frame::new(internal::NET_REGISTER).num(netw::tag::PARTITION, partition_of(2)),
        Frame::new(internal::NET_REGISTER).num(netw::tag::PARTITION, 0),
        Frame::new(internal::NET_REGISTER).bytes(tag::DATA, &encoded),
    ] {
        let r = one.answer(&q).unwrap();
        assert_eq!(r.kind, kind::RESULT, "{r:?}");
    }
    assert_eq!(one.tokens.len(), 3);
    assert!(net.unregister(&theirs).unwrap(), "session 2's registration is still there");
    let theirs = two.answer(&Frame::new(internal::NET_REGISTER)).unwrap().field(tag::DATA).unwrap().to_vec();
    let release = Frame::new(internal::NET_RELEASE).bytes(tag::DATA, &theirs);
    assert_eq!(code_of(&one.answer(&release).unwrap()), Some(code::REFUSED), "another session's token");
    assert_eq!(two.answer(&release).unwrap().kind, kind::RESULT);
    for k in [internal::RESTORE, internal::STATE, internal::END, 247, 300] {
        assert_eq!(code_of(&one.answer(&Frame::new(k)).unwrap()), Some(code::REFUSED), "kind {k}");
    }
    // session 1's three registrations were all in its own partition
    let dropped = net.end_session(1).unwrap();
    assert!(dropped, "session 1's partition was opened");
    for t in one.tokens.keys() {
        assert!(!net.unregister(t).unwrap(), "dropped with session 1's partition");
    }
}

#[test]
fn only_the_sessions_own_pages_are_taken_from_its_process_or_given_back() {
    let (s, _dir) = shared("owners", false, |_| {});
    s.owners.lock().unwrap().insert(7, 2);
    s.pages.store(1, Ordering::SeqCst);
    let mut p = Proxy::new(1, &s);
    let mine = p.new_page().get_num(tag::PAGE).unwrap() as u32;
    assert_eq!(s.pages.load(Ordering::SeqCst), 2);
    p.adopt(vec![page(7), page(mine), page(mine), page(99)]);
    assert_eq!(p.pages, [page(mine)], "another session's page, a duplicate and an unknown number are not taken");
    p.release(7);
    p.release(99);
    assert_eq!(s.owners.lock().unwrap().get(&7), Some(&2));
    assert_eq!(s.pages.load(Ordering::SeqCst), 2, "nothing given back for pages not the session's");
    p.adopt(Vec::new());
    p.release(mine);
    assert_eq!(s.pages.load(Ordering::SeqCst), 1, "its own page given back once");
    assert!(!s.owners.lock().unwrap().contains_key(&mine));
}

#[test]
fn the_host_holds_a_session_to_its_page_limit() {
    let (s, _dir) = shared("cap", false, |c| c.max_pages = 2);
    let mut p = Proxy::new(1, &s);
    assert!(p.new_page().get_num(tag::PAGE).is_some());
    p.adopt(Vec::new());
    p.new_page();
    p.new_page();
    let r = p.new_page();
    assert_eq!(code_of(&r), Some(code::TOO_MANY_PAGES), "{r:?}");
    assert_eq!(s.pages.load(Ordering::SeqCst), 2);
}

#[test]
fn a_process_that_answers_but_hangs_on_its_page_list_is_killed_and_replaced() {
    let (s, _dir) = shared("hung", false, |c| {
        c.hang = Duration::from_millis(300);
        c.isolate = Some(PathBuf::from("/usr/bin/true"));
    });
    let (host, mut fake) = UnixStream::pair().unwrap();
    let sleeper = std::process::Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let pid = sleeper.id() as i32;
    let mut p = Proxy::new(1, &s);
    p.child = Some(Content::fake(sleeper, host));
    let t = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        let req = read_until(&mut fake, &mut buf, deadline).unwrap().unwrap();
        assert_eq!(req.kind, kind::PAGES);
        write(&mut fake, &Frame::new(kind::RESULT).str(tag::TEXT, "no pages open\n")).unwrap();
        let state = read_until(&mut fake, &mut buf, deadline).unwrap().unwrap();
        assert_eq!(state.kind, internal::STATE);
        std::thread::sleep(Duration::from_millis(600));
        // the late answer goes nowhere: the channel went with the process
        let late = write(&mut fake, &Frame::new(kind::RESULT).str(tag::TEXT, "late"));
        late.is_err() || read_until(&mut fake, &mut buf, Instant::now() + Duration::from_millis(200)).is_err()
    });
    let r = p.forward(&Frame::new(kind::PAGES));
    assert_eq!(r.kind, kind::RESULT);
    assert_eq!(p.restarts, 1, "a new content process");
    // SAFETY: signal 0 only checks that the process exists
    assert_ne!(unsafe { kill(pid, 0) }, 0, "the hung process was killed");
    assert!(t.join().unwrap(), "the late answer reached no one");
}
