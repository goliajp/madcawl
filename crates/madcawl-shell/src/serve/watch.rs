//! The watchdog of a session's request loop, which is the main thread of
//! a content process. Each request is timed and split into named stages
//! (parse, style, layout, agent, network, paint); a request that
//! took longer than the slow threshold gets one line naming the stages
//! that took the time, and while a request is still running, a thread of
//! the watchdog's own says so once at the slow threshold and once more at
//! the stuck threshold, naming the stage it is in. A loop that is idle
//! and a loop that is wedged would otherwise leave the same trace in the
//! log: nothing. The watchdog thread sleeps on a condition variable
//! while no request runs, so an idle loop costs no wakeups.
//!
//! The watchdog only reports. A content process that does not answer at
//! all is ended by its host (`proxy`), which then loads its pages again.

use std::cell::RefCell;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The most stages a request is split into; later ones are counted in
/// the last.
const MAX_STAGES: usize = 12;

/// Where the watchdog's lines go.
pub type Sink = Box<dyn Fn(&str) + Send + Sync>;

/// What the watchdog checks a running request against.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub slow: Duration,
    pub stuck: Duration,
}

impl Limits {
    /// A slow request is one over `slow`; stuck is ten times that, and at
    /// least ten seconds.
    pub fn new(slow: Duration) -> Limits {
        Limits { slow, stuck: (slow * 10).max(Duration::from_secs(10)) }
    }
}

struct Busy {
    seq: u64,
    what: &'static str,
    started: Instant,
    stage: &'static str,
    stage_at: Instant,
    stages: [(&'static str, Duration); MAX_STAGES],
    n: usize,
    told: u8,
}

struct State {
    seq: u64,
    busy: Option<Busy>,
    done: bool,
}

struct Inner {
    state: Mutex<State>,
    cv: Condvar,
    limits: Limits,
    sink: Sink,
    /// Called while a request runs, every `limits.slow / 4` at most
    every: Option<Box<dyn Fn() + Send + Sync>>,
}

#[derive(Clone)]
pub struct Watch(Arc<Inner>);

thread_local! {
    static CURRENT: RefCell<Option<Watch>> = const { RefCell::new(None) };
}

/// Marks the start of stage `name` of the request the calling thread's
/// watch is timing; nothing when the thread has no watch.
pub fn stage(name: &'static str) {
    CURRENT.with(|c| {
        if let Some(w) = c.borrow().as_ref() {
            w.stage(name);
        }
    });
}

impl Watch {
    /// A watch reporting to `sink`, with its thread started; `every`, if
    /// given, runs on that thread while a request runs.
    pub fn start(limits: Limits, sink: Sink, every: Option<Box<dyn Fn() + Send + Sync>>) -> std::io::Result<Watch> {
        let state = State { seq: 0, busy: None, done: false };
        let w = Watch(Arc::new(Inner { state: Mutex::new(state), cv: Condvar::new(), limits, sink, every }));
        let theirs = w.0.clone();
        std::thread::Builder::new().name("watchdog".into()).spawn(move || watchdog(&theirs))?;
        Ok(w)
    }

    /// The watchdog of session `id`'s request loop, which runs on the
    /// calling thread; none when its thread cannot start.
    pub(crate) fn for_session(id: u32, slow: Duration) -> Option<Watch> {
        let sink: Sink = Box::new(move |line| eprintln!("madcawl-shell: session {id}: watchdog: {line}"));
        match Watch::start(Limits::new(slow), sink, None) {
            Ok(w) => {
                w.install();
                Some(w)
            }
            Err(e) => {
                eprintln!("madcawl-shell: session {id}: no watchdog: {e}");
                None
            }
        }
    }

    /// Makes this the watch `stage` reports to on the calling thread.
    pub fn install(&self) {
        CURRENT.with(|c| *c.borrow_mut() = Some(self.clone()));
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A request of kind `what` starts.
    pub fn begin(&self, what: &'static str) {
        let now = Instant::now();
        let mut s = self.lock();
        s.seq += 1;
        let b = Busy { seq: s.seq, what, started: now, stage: "request", stage_at: now, stages: [("", Duration::ZERO); MAX_STAGES], n: 0, told: 0 };
        s.busy = Some(b);
        drop(s);
        self.0.cv.notify_all();
    }

    pub fn stage(&self, name: &'static str) {
        let now = Instant::now();
        if let Some(b) = self.lock().busy.as_mut() {
            close_stage(b, now);
            b.stage = name;
            b.stage_at = now;
        }
    }

    /// Ends the watchdog thread; `stage` on this thread stops reporting.
    pub fn stop(&self) {
        self.lock().done = true;
        self.0.cv.notify_all();
        CURRENT.with(|c| c.borrow_mut().take());
    }

    /// The request ends; a line on it when it was slow.
    pub fn end(&self) {
        let now = Instant::now();
        let Some(mut b) = self.lock().busy.take() else { return };
        close_stage(&mut b, now);
        let took = now - b.started;
        if took < self.0.limits.slow {
            return;
        }
        let mut stages: Vec<_> = b.stages[..b.n].to_vec();
        stages.sort_by_key(|s| std::cmp::Reverse(s.1));
        let parts: Vec<String> = stages.iter().map(|(n, d)| format!("{n} {} ms", d.as_millis())).collect();
        (self.0.sink)(&format!("slow request {}: {} ms ({})", b.what, took.as_millis(), parts.join(", ")));
    }
}

/// Adds the time since the stage began to that stage's total.
fn close_stage(b: &mut Busy, now: Instant) {
    let d = now - b.stage_at;
    if let Some(s) = b.stages[..b.n].iter_mut().find(|s| s.0 == b.stage) {
        s.1 += d;
    } else if b.n < MAX_STAGES {
        b.stages[b.n] = (b.stage, d);
        b.n += 1;
    } else {
        b.stages[MAX_STAGES - 1].1 += d;
    }
}

/// The watchdog thread: asleep while no request runs; while one does, it
/// wakes at the thresholds (and every quarter of the slow one when there
/// is a periodic check) and reports a request that has passed one.
fn watchdog(w: &Arc<Inner>) {
    let mut s = w.state.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if s.done {
            return;
        }
        let Some(b) = s.busy.as_mut() else {
            s = w.cv.wait(s).unwrap_or_else(|e| e.into_inner());
            continue;
        };
        let elapsed = b.started.elapsed();
        let (limit, word) = match b.told {
            0 => (w.limits.slow, "still running"),
            1 => (w.limits.stuck, "not responding"),
            _ => (Duration::MAX, ""),
        };
        if elapsed >= limit {
            b.told += 1;
            let line = format!("{word}: request {} for {} ms, in stage {}", b.what, elapsed.as_millis(), b.stage);
            (w.sink)(&line);
            continue;
        }
        let seq = b.seq;
        let mut wait = limit.saturating_sub(elapsed);
        if w.every.is_some() {
            wait = wait.min((w.limits.slow / 4).max(Duration::from_millis(10)));
        }
        s = w.cv.wait_timeout(s, wait).unwrap_or_else(|e| e.into_inner()).0;
        if let (Some(f), Some(b)) = (&w.every, s.busy.as_ref())
            && b.seq == seq
        {
            drop(s);
            f();
            s = w.state.lock().unwrap_or_else(|e| e.into_inner());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn watch(slow_ms: u64, stuck_ms: u64) -> (Watch, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let sink: Sink = Box::new(move |l| drop(tx.lock().unwrap().send(l.to_owned())));
        let limits = Limits { slow: Duration::from_millis(slow_ms), stuck: Duration::from_millis(stuck_ms) };
        (Watch::start(limits, sink, None).unwrap(), rx)
    }

    #[test]
    fn a_fast_request_says_nothing() {
        let (w, rx) = watch(200, 400);
        w.begin("read");
        w.stage("parse");
        w.end();
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }

    #[test]
    fn a_running_request_is_reported_with_its_stage_then_summed_by_stage() {
        let (w, rx) = watch(40, 120);
        w.install();
        w.begin("open");
        stage("parse");
        std::thread::sleep(Duration::from_millis(20));
        stage("style");
        let still = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(still.starts_with("still running: request open for ") && still.ends_with("in stage style"), "{still}");
        let stuck = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(stuck.starts_with("not responding: request open") && stuck.ends_with("in stage style"), "{stuck}");
        w.end();
        let slow = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(slow.starts_with("slow request open: "), "{slow}");
        let at = |s: &str| slow.find(s).unwrap();
        assert!(at("style") < at("parse"), "the stage that took longest first: {slow}");
    }

    #[test]
    fn a_periodic_check_runs_only_while_a_request_does() {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let every: Box<dyn Fn() + Send + Sync> = Box::new(move || {
            let _ = tx.lock().unwrap().send(());
        });
        let limits = Limits { slow: Duration::from_millis(40), stuck: Duration::from_secs(60) };
        let w = Watch::start(limits, Box::new(|_| {}), Some(every)).unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err(), "idle: no check");
        w.begin("act");
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        w.end();
        while rx.recv_timeout(Duration::from_millis(60)).is_ok() {}
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err(), "idle again: no check");
    }
}
