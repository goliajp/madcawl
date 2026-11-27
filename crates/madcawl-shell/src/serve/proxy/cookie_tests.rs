//! A content process's `document.cookie` against a network service that
//! stalls: the host gives up its registration when the content process
//! stops waiting, so a document's script waits about its cookie budget
//! in all, not the host's own waits.

use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use madcawl_net::wire::{Frame as NetFrame, kind as netk, tag as nett};

use super::Proxy;
use super::tests::Dir;
use crate::serve::cookies::{Cookies, NetCookies};
use crate::serve::link::{Link, read_until, write};
use crate::serve::netctl::Control;
use crate::serve::{AuditLog, Config, Net, Shared};

/// A host's network whose control channel answers each REGISTER after
/// `delay` (never, when `None`) and each UNREGISTER at once, counting
/// those, and whose service takes connections and never says HELLO.
fn stalled(name: &str, delay: Option<Duration>) -> (Shared, Arc<AtomicUsize>, Dir) {
    let d = Dir(std::env::temp_dir().join(format!("mc-cookie-{}-{name}", std::process::id())));
    std::fs::create_dir_all(&d.0).unwrap();
    let sock = d.0.join("s.net");
    let listener = UnixListener::bind(&sock).unwrap();
    std::thread::spawn(move || {
        let held: Vec<UnixStream> = listener.incoming().map_while(Result::ok).collect();
        drop(held);
    });
    let (host, mut peer) = UnixStream::pair().unwrap();
    let unregistered = Arc::new(AtomicUsize::new(0));
    let u = unregistered.clone();
    std::thread::spawn(move || {
        while let Ok(Some(f)) = NetFrame::read_from(&mut peer) {
            let id = f.get_num(nett::ID).unwrap();
            if f.kind == netk::UNREGISTER {
                u.fetch_add(1, Ordering::SeqCst);
            } else if let Some(delay) = delay {
                std::thread::sleep(delay);
            } else {
                continue;
            }
            let _ = NetFrame::new(netk::REGISTERED).num(nett::ID, id).bytes(nett::TOKEN, b"0123456789abcdef").write_to(&mut peer);
        }
    });
    let net = Arc::new(Net::new(&sock, Control::new(host, Duration::from_secs(10)).unwrap(), None, None));
    let log = AuditLog::open(&d.0.join("audit.log")).unwrap();
    (Shared::new(Config::new(d.0.join("s")), log, Some(net), "run".into(), None), unregistered, d)
}

/// How long a content process's script spends on `n` cookie reads and
/// writes, the host answering its questions as `Proxy` does.
fn script_time(s: &Shared, sock: &std::path::Path, n: usize) -> Duration {
    let (content, mut host) = UnixStream::pair().unwrap();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let mut p = Proxy::new(1, s);
            let mut buf = Vec::new();
            while let Ok(Some(q)) = read_until(&mut host, &mut buf, Instant::now() + Duration::from_secs(60)) {
                if let Some(r) = p.answer(&q) {
                    write(&mut host, &r).unwrap();
                }
            }
        });
        let net = Arc::new(Net::through(Arc::new(Link::new(content)), sock, None));
        let mut jar = NetCookies::new(net, 1);
        let t0 = Instant::now();
        for _ in 0..n {
            assert_eq!(jar.get("https://a.example/"), "");
            jar.set("https://a.example/", "b=2");
        }
        jar.idle();
        let took = t0.elapsed();
        drop(jar);
        took
    })
}

#[test]
fn a_registration_the_service_never_answers_costs_a_content_process_one_wait() {
    let (s, _, d) = stalled("never", None);
    let took = script_time(&s, &d.0.join("s.net"), 200);
    assert!(took < Duration::from_millis(3500), "{took:?}");
}

#[test]
fn a_late_registration_and_a_refused_connection_cost_a_content_process_one_wait() {
    let (s, unregistered, d) = stalled("late", Some(Duration::from_millis(900)));
    let took = script_time(&s, &d.0.join("s.net"), 200);
    assert!(took < Duration::from_millis(3500), "{took:?}");
    // the release with no time left is sent without waiting for its reply
    let t0 = Instant::now();
    while unregistered.load(Ordering::SeqCst) == 0 && t0.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(unregistered.load(Ordering::SeqCst), 1, "the registration was released");
}
