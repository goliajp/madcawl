//! The time limit on JavaScriptCore: script that runs past it is
//! stopped, reported as an exception nothing caught, and the page's
//! next script runs in the same isolate. The limit counts the time
//! script runs, not time a host function blocks.

use std::time::{Duration, Instant};

use madcawl_bindings::{CookieStore, DomHost, Note};
use madcawl_dom::Dom;
use madcawl_js::{Control, Engine, Halt, Interrupt, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

const LIMIT: Duration = Duration::from_millis(200);
const STOPPED: &str = "uncaught exception: script stopped: it ran past the time limit";
const STOPPED_IN_JOB: &str = "uncaught exception: script stopped: a promise job ran past the time limit; the jobs queued after it were dropped";

fn page(html: &str) -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    e.set_time_limit(Some(LIMIT)).unwrap();
    let mut h = DomHost::new(&mut e, Dom::new());
    assert!(h.set_url("http://a.test/"));
    h.install_window(&mut e);
    h.use_virtual_time();
    run(&mut e, &mut h, "globalThis.log = [];");
    h.parse_document(&mut e, html.as_bytes());
    (e, h)
}

fn run(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) {
    h.run_script(e, &DomString::from_wtf8(src.as_bytes()), &DomString::from_wtf8(b"t"));
}

fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"t");
    let realm = h.realm;
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    assert!(!Jsc::is_thrown(r.get()), "{src} threw");
    let mut b = Vec::new();
    e.to_string(h, r.get()).unwrap().write_wtf8(&mut b);
    e.release(r);
    String::from_utf8(b).unwrap()
}

fn notes(h: &mut DomHost<Jsc>) -> Vec<String> {
    h.take_notes().iter().map(Note::to_string).collect()
}

/// `src` run as a classic script: how long it took, and the notes.
fn timed(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> (Duration, Vec<String>) {
    let t0 = Instant::now();
    run(e, h, src);
    (t0.elapsed(), notes(h))
}

fn stopped_in_time(took: Duration) {
    assert!(took >= LIMIT.mul_f64(0.9) && took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn an_endless_loop_is_stopped_and_the_page_goes_on() {
    let (mut e, mut h) = page("<p>x</p>");
    let (took, n) = timed(&mut e, &mut h, "Promise.resolve().then(() => log.push('kept')); log.push('a'); for (;;) {}");
    stopped_in_time(took);
    assert_eq!(n, [STOPPED]);
    assert_eq!(e.halt(), Halt::Running);
    run(&mut e, &mut h, "log.push('next')");
    assert_eq!(eval(&mut e, &mut h, "log.join()"), "a,kept,next", "the stopped script's microtask runs at its checkpoint");
    assert!(notes(&mut h).is_empty());
    h.close(&mut e);
}

#[test]
fn an_exception_whose_to_string_never_returns_is_stopped() {
    let (mut e, mut h) = page("");
    let (took, n) = timed(&mut e, &mut h, "throw { toString() { for (;;) {} } }");
    stopped_in_time(took);
    assert_eq!(n, [STOPPED]);
    run(&mut e, &mut h, "log.push('next')");
    assert_eq!(eval(&mut e, &mut h, "log.join()"), "next");
    h.close(&mut e);
}

#[test]
fn a_microtask_that_never_returns_is_stopped_at_its_checkpoint() {
    let (mut e, mut h) = page("");
    let src = "Promise.resolve().then(() => { log.push('m1'); for (;;) {} }); Promise.resolve().then(() => log.push('m2')); log.push('sync')";
    let (took, n) = timed(&mut e, &mut h, src);
    stopped_in_time(took);
    assert_eq!(n, [STOPPED_IN_JOB]);
    run(&mut e, &mut h, "log.push('next')");
    assert_eq!(eval(&mut e, &mut h, "log.join()"), "sync,m1,next", "the job queued after the stopped one is dropped");
    h.close(&mut e);
}

#[test]
fn a_timer_whose_callback_never_returns_is_stopped_once_and_other_timers_run() {
    let (mut e, mut h) = page("");
    run(&mut e, &mut h, "setInterval(() => { log.push('spin'); for (;;) {} }, 1); setTimeout(() => log.push('later'), 50)");
    let t0 = Instant::now();
    while h.run_tasks(&mut e, 100) != 0 {}
    stopped_in_time(t0.elapsed());
    assert_eq!(notes(&mut h), [STOPPED], "the interval is not run again");
    assert_eq!(eval(&mut e, &mut h, "log.join()"), "spin,later");
    h.close(&mut e);
}

#[test]
fn a_listener_stopped_under_a_script_that_then_returns_is_not_called_a_job() {
    let (mut e, mut h) = page("<div id=d></div>");
    let src = "var d = document.getElementById('d'); d.addEventListener('x', () => { for (;;) {} }); \
               d.dispatchEvent(new Event('x')); log.push('after');";
    let (took, n) = timed(&mut e, &mut h, src);
    stopped_in_time(took);
    assert_eq!(n, [STOPPED]);
    run(&mut e, &mut h, "log.push('next')");
    assert!(eval(&mut e, &mut h, "log.join()").ends_with("next"));
    h.close(&mut e);
}

#[test]
fn the_parser_runs_the_scripts_after_one_that_was_stopped() {
    let html = "<script>log.push('1')</script><script>for (;;) {}</script><script>log.push('3')</script>\
                <script>addEventListener('load', () => log.push('load'))</script>";
    let (mut e, mut h) = page(html);
    while h.run_tasks(&mut e, 100) != 0 {}
    assert!(h.is_loaded());
    assert_eq!(notes(&mut h), [STOPPED]);
    assert_eq!(eval(&mut e, &mut h, "log.join()"), "1,3,load");
    h.close(&mut e);
}

#[test]
fn a_long_script_under_the_limit_runs_to_its_end() {
    let (mut e, mut h) = page("");
    let src = "var t = Date.now(), n = 0; while (Date.now() - t < 120) n++; log.push('done')";
    let (took, n) = timed(&mut e, &mut h, src);
    assert!(took >= Duration::from_millis(110), "{took:?}");
    assert!(n.is_empty(), "{n:?}");
    assert_eq!(eval(&mut e, &mut h, "log.join()"), "done");
    h.close(&mut e);
}

/// A jar that takes 60 ms to answer, as the network service may.
struct Slow;

impl CookieStore for Slow {
    fn cookies(&mut self, _: &str) -> String {
        std::thread::sleep(Duration::from_millis(60));
        "a=1".into()
    }

    fn set_cookie(&mut self, _: &str, _: &str) {}
}

#[test]
fn time_a_host_function_blocks_does_not_count() {
    let (mut e, mut h) = page("");
    h.set_cookie_store(Box::new(Slow));
    let (took, n) = timed(&mut e, &mut h, "for (let i = 0; i < 8; i++) log.push(document.cookie)");
    assert!(took >= Duration::from_millis(480), "{took:?}");
    assert!(n.is_empty(), "{n:?}");
    assert_eq!(eval(&mut e, &mut h, "log.length"), "8");
    h.close(&mut e);
}

#[test]
fn no_limit_without_the_watchdog_and_none_once_lifted() {
    assert!(Jsc::without_watchdog().set_time_limit(Some(LIMIT)).is_err());
    let (mut e, mut h) = page("");
    e.set_time_limit(None).unwrap();
    let (_, n) = timed(&mut e, &mut h, "var t = Date.now(); while (Date.now() - t < 300) {} log.push('done')");
    assert!(n.is_empty(), "{n:?}");
    assert_eq!(eval(&mut e, &mut h, "log.join()"), "done");
    h.close(&mut e);
}

/// A jar whose read asks the isolate to abort, as another thread may at
/// any moment.
struct Aborts<I: Interrupt>(I);

impl<I: Interrupt> CookieStore for Aborts<I> {
    fn cookies(&mut self, _: &str) -> String {
        self.0.abort().unwrap();
        String::new()
    }

    fn set_cookie(&mut self, _: &str, _: &str) {}
}

#[test]
fn an_abort_the_watchdog_never_acted_on_is_dropped_not_noted() {
    let (mut e, mut h) = page("");
    h.set_cookie_store(Box::new(Aborts(e.interrupt_handle())));
    // the script ends before the watchdog's next check sees the request
    let (_, n) = timed(&mut e, &mut h, "document.cookie; Promise.resolve().then(() => log.push('job')); log.push('sync')");
    assert!(n.is_empty(), "{n:?}");
    assert_eq!(e.halt(), Halt::Running);
    run(&mut e, &mut h, "log.push('next')");
    assert_eq!(eval(&mut e, &mut h, "log.join()"), "sync,job,next");
    h.close(&mut e);
}

#[test]
fn an_abort_another_thread_asks_for_is_noted_without_the_limit() {
    let (mut e, mut h) = page("");
    e.set_time_limit(None).unwrap();
    let i = e.interrupt_handle();
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        i.abort().unwrap();
    });
    let (_, n) = timed(&mut e, &mut h, "for (;;) {}");
    t.join().unwrap();
    assert_eq!(n, ["uncaught exception: script stopped"]);
    h.close(&mut e);
}
