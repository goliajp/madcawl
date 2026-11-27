//! `document.cookie` against network services that stall, answer slowly
//! or go away: a document's script waits a bounded time in all and does
//! not reconnect once its cookies have failed.

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::super::netctl::Control;
use super::*;

/// What a fake service does with one connection: how many frames to
/// answer after HELLO (none past it), how long to wait before each
/// answer, and whether to answer HELLO at all.
#[derive(Clone, Copy)]
struct Serve {
    hello: bool,
    answers: usize,
    delay: Duration,
}

#[derive(Default)]
struct Counts {
    registered: AtomicUsize,
    unregistered: AtomicUsize,
    answered: AtomicUsize,
}

/// A network whose control channel registers and releases at once, and
/// whose service treats its `n`th connection as `plan(n)` says.
fn fake(name: &str, plan: fn(usize) -> Serve) -> (Arc<Net>, Arc<Counts>, PathBuf) {
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mck-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("s");
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock).unwrap();
    let counts = Arc::new(Counts::default());
    let (host, mut peer) = UnixStream::pair().unwrap();
    let c = counts.clone();
    std::thread::spawn(move || {
        while let Ok(Some(f)) = Frame::read_from(&mut peer) {
            let id = f.get_num(tag::ID).unwrap();
            let n = if f.kind == kind::REGISTER { &c.registered } else { &c.unregistered };
            n.fetch_add(1, Ordering::SeqCst);
            let _ = Frame::new(kind::REGISTERED).num(tag::ID, id).bytes(tag::TOKEN, b"0123456789abcdef").write_to(&mut peer);
        }
    });
    let c = counts.clone();
    std::thread::spawn(move || {
        for (n, s) in listener.incoming().enumerate() {
            let (s, c) = (s.unwrap(), c.clone());
            std::thread::spawn(move || serve(s, plan(n), &c));
        }
    });
    let net = Net::new(&sock, Control::new(host, Duration::from_secs(10)).unwrap(), None, None);
    (Arc::new(net), counts, dir)
}

fn serve(mut s: UnixStream, plan: Serve, c: &Counts) {
    let Ok(Some(_hello)) = Frame::read_from(&mut s) else { return };
    if !plan.hello {
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let _ = Frame::new(kind::HELLO).num(tag::ID, 0).num(tag::OK, 1).write_to(&mut s);
    let mut answered = 0;
    while let Ok(Some(f)) = Frame::read_from(&mut s) {
        if answered == plan.answers {
            // a stalled service holds the connection; one that went away closes it
            if plan.delay.is_zero() {
                return;
            }
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        std::thread::sleep(plan.delay);
        answered += 1;
        c.answered.fetch_add(1, Ordering::SeqCst);
        let r = Frame::new(kind::COOKIES).num(tag::ID, f.get_num(tag::ID).unwrap()).str(tag::TEXT, "a=1");
        let _ = r.write_to(&mut s);
    }
}

/// `n` calls of each kind on `jar`, and how long they took.
fn hammer(jar: &mut NetCookies, n: usize) -> Duration {
    let t0 = Instant::now();
    for _ in 0..n {
        assert_eq!(jar.get("https://a.example/"), "");
        jar.set("https://a.example/", "b=2");
    }
    t0.elapsed()
}

fn settle(n: &AtomicUsize, want: usize) -> usize {
    let t0 = Instant::now();
    while n.load(Ordering::SeqCst) < want && t0.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(5));
    }
    n.load(Ordering::SeqCst)
}

#[test]
fn a_service_that_never_takes_the_connection_costs_one_wait_and_one_registration() {
    let (net, counts, dir) = fake("hello", |_| Serve { hello: false, answers: 0, delay: Duration::ZERO });
    let mut jar = NetCookies::new(net, 1);
    let took = hammer(&mut jar, 500);
    assert!(took < Duration::from_millis(2500), "{took:?}");
    assert_eq!(counts.registered.load(Ordering::SeqCst), 1, "no connection tried after the first failed");
    assert_eq!(settle(&counts.unregistered, 1), 1, "the registration the connection was refused on is released");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_service_that_never_answers_costs_one_wait() {
    let (net, counts, dir) = fake("stall", |_| Serve { hello: true, answers: 0, delay: Duration::from_millis(1) });
    let mut jar = NetCookies::new(net, 1);
    let took = hammer(&mut jar, 500);
    assert!(took < Duration::from_millis(2500), "{took:?}");
    assert_eq!(counts.registered.load(Ordering::SeqCst), 1);
    assert_eq!(settle(&counts.unregistered, 1), 1);
    drop(jar);
    assert_eq!(settle(&counts.unregistered, 2), 1, "nothing left to release");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_slow_service_is_waited_for_until_the_document_s_budget_is_spent() {
    let (net, counts, dir) = fake("slow", |_| Serve { hello: true, answers: usize::MAX, delay: Duration::from_millis(400) });
    let mut jar = NetCookies::new(net, 1);
    let t0 = Instant::now();
    let seen: Vec<String> = (0..40).map(|_| jar.get("https://a.example/")).collect();
    let took = t0.elapsed();
    let answered = seen.iter().filter(|s| *s == "a=1").count();
    assert!((5..=8).contains(&answered), "{answered} answers in {took:?}");
    assert!(took < Duration::from_millis(4000), "{took:?}");
    assert_eq!(counts.registered.load(Ordering::SeqCst), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_connection_that_went_away_gets_one_more_try_on_a_new_one() {
    // each connection answers once, then closes on the next frame
    let (net, counts, dir) = fake("away", |_| Serve { hello: true, answers: 1, delay: Duration::ZERO });
    let mut jar = NetCookies::new(net, 1);
    assert_eq!(jar.get("https://a.example/"), "a=1");
    assert_eq!(jar.get("https://a.example/"), "a=1", "asked again on a new connection");
    assert_eq!(counts.registered.load(Ordering::SeqCst), 2);
    assert_eq!(counts.answered.load(Ordering::SeqCst), 2);
    assert_eq!(settle(&counts.unregistered, 1), 1, "the closed connection's registration is released");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_turn_s_end_releases_the_connection_and_the_next_turn_makes_one() {
    let (net, counts, dir) = fake("idle", |_| Serve { hello: true, answers: usize::MAX, delay: Duration::ZERO });
    let mut jar = NetCookies::new(net, 1);
    assert_eq!(jar.get("https://a.example/"), "a=1");
    assert_eq!(jar.get("https://a.example/"), "a=1");
    jar.idle();
    assert_eq!(settle(&counts.unregistered, 1), 1);
    jar.idle();
    assert_eq!(jar.get("https://a.example/"), "a=1");
    assert_eq!(counts.registered.load(Ordering::SeqCst), 2);
    drop(jar);
    assert_eq!(settle(&counts.unregistered, 2), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_page_that_reads_its_cookies_every_turn_never_runs_out_while_the_service_answers_promptly() {
    // 10 000 calls of 0.4 ms each take longer in all than the budget
    let (net, counts, dir) = fake("prompt", |_| Serve { hello: true, answers: usize::MAX, delay: Duration::from_micros(400) });
    let mut jar = NetCookies::new(net, 1);
    for n in 0..10_000 {
        assert_eq!(jar.get("https://a.example/"), "a=1", "call {n}");
        if n % 100 == 99 {
            jar.idle();
        }
    }
    assert!(!jar.failed);
    assert_eq!(counts.registered.load(Ordering::SeqCst), 100, "a connection a turn");
    std::fs::remove_dir_all(dir).unwrap();
}
