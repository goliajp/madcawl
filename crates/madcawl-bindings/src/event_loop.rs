//! The event loop of one window: a task queue and the timers, each task
//! followed by a microtask checkpoint.
//!
//! Time is real, or virtual for a page no one watches: with nothing
//! else to do, virtual time jumps to the next timer, so a page whose
//! script waits on timers finishes as fast as its script runs. Timers
//! due at the same moment run in the order they were set.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Instant;

use madcawl_js::{Engine, Halt, Owned, Root, ScriptOrigin};
use madcawl_str::DomString;

use crate::host::DomHost;

/// Nested timers past this depth wait at least `MIN_NESTED_MS`.
const NESTING_CLAMP: u32 = 5;
const MIN_NESTED_MS: u32 = 4;
const NS_PER_MS: u64 = 1_000_000;
/// How long the collection after a task may take before it yields.
pub(crate) const COLLECT_SLICE_NS: u64 = 2 * NS_PER_MS;
/// The first id of the abort timers; those of script's are below.
const ABORT_IDS: u32 = 1 << 31;

pub(crate) struct Timer<E: Engine> {
    pub id: u32,
    /// A function, or a string compiled when the timer fires.
    pub handler: Root<E>,
    pub args: Vec<Root<E>>,
    /// The interval asked for, in milliseconds.
    pub repeat: Option<u32>,
    pub nesting: u32,
    /// The timer of `AbortSignal.timeout`: the handler is the signal's
    /// wrapper, and its id is above any `clearTimeout` reaches.
    pub abort: bool,
}

/// A queued task: a callback script queued, or one of the host's own.
pub(crate) enum Task<E: Engine> {
    Call(Root<E>),
    Script(crate::scripts::ScriptTask),
}

pub enum Clock {
    Real(Instant),
    /// Nanoseconds since the loop began.
    Virtual(u64),
}

pub struct EventLoop<E: Engine> {
    timers: BTreeMap<(u64, u64), Timer<E>>,
    /// Each live timer's key; a running timer keeps its entry, which
    /// clearing it removes.
    by_id: HashMap<u32, (u64, u64)>,
    next_id: u32,
    /// The next id of an abort timer, from 2^31.
    next_abort: u32,
    seq: u64,
    pub(crate) tasks: VecDeque<Task<E>>,
    clock: Clock,
    /// The nesting level of the timer running, 0 outside one.
    nesting: u32,
}

impl<E: Engine> EventLoop<E> {
    pub(crate) fn new() -> Self {
        EventLoop {
            timers: BTreeMap::new(),
            by_id: HashMap::new(),
            next_id: 1,
            next_abort: ABORT_IDS,
            seq: 0,
            tasks: VecDeque::new(),
            clock: Clock::Real(Instant::now()),
            nesting: 0,
        }
    }

    pub(crate) fn now_ns(&self) -> u64 {
        match self.clock {
            Clock::Real(t0) => t0.elapsed().as_nanos() as u64,
            Clock::Virtual(t) => t,
        }
    }

    /// A timer of `ms` milliseconds, already a Web IDL `long`.
    pub(crate) fn add_timer(&mut self, handler: Root<E>, args: Vec<Root<E>>, ms: i32, repeat: bool) -> u32 {
        let ms = ms.max(0) as u32;
        let id = self.take_id(false);
        let t = Timer { id, handler, args, repeat: repeat.then_some(ms), nesting: self.nesting + 1, abort: false };
        self.schedule(t, u64::from(ms));
        id
    }

    /// A timer that aborts the signal whose wrapper is `signal` after
    /// `ms` milliseconds.
    pub(crate) fn add_abort_timer(&mut self, signal: Root<E>, ms: u64) {
        let id = self.take_id(true);
        let t = Timer { id, handler: signal, args: Vec::new(), repeat: None, nesting: 0, abort: true };
        self.schedule(t, ms);
    }

    /// The next id that no live timer has, wrapping: from 1 to 2^31 - 1
    /// for script's, so a page may keep an interval for as long as it
    /// likes, and from 2^31 up for abort timers.
    fn take_id(&mut self, abort: bool) -> u32 {
        loop {
            let (next, first, last) = if abort { (&mut self.next_abort, ABORT_IDS, u32::MAX) } else { (&mut self.next_id, 1, ABORT_IDS - 1) };
            let id = *next;
            *next = if id == last { first } else { id + 1 };
            if !self.by_id.contains_key(&id) {
                return id;
            }
        }
    }

    fn schedule(&mut self, t: Timer<E>, ms: u64) {
        let ms = if t.nesting > NESTING_CLAMP { ms.max(u64::from(MIN_NESTED_MS)) } else { ms };
        let key = (self.now_ns().saturating_add(ms.saturating_mul(NS_PER_MS)), self.seq);
        self.seq += 1;
        self.by_id.insert(t.id, key);
        self.timers.insert(key, t);
    }

    /// Take timer `id` off the list; `None` when there is none, or when
    /// it is the one running, which then is not run again.
    pub(crate) fn remove_timer(&mut self, id: u32) -> Option<Timer<E>> {
        let key = self.by_id.remove(&id)?;
        self.timers.remove(&key)
    }

    /// The next timer that is due, or that virtual time can jump to.
    fn take_due(&mut self) -> Option<Timer<E>> {
        let (&key, _) = self.timers.first_key_value()?;
        let now = self.now_ns();
        match &mut self.clock {
            Clock::Real(_) if key.0 > now => return None,
            Clock::Virtual(t) => *t = (*t).max(key.0),
            Clock::Real(_) => {}
        }
        Some(self.timers.remove(&key).expect("the first timer"))
    }

    pub fn pending(&self) -> usize {
        self.timers.len() + self.tasks.len()
    }

    /// Nanoseconds until the next timer, for a host that sleeps on real
    /// time; `None` with no timer.
    pub fn next_due_in(&self) -> Option<u64> {
        let (&(due, _), _) = self.timers.first_key_value()?;
        Some(due.saturating_sub(self.now_ns()))
    }
}

impl<E: Engine> DomHost<E> {
    /// Let time jump to the next timer whenever nothing else is ready.
    pub fn use_virtual_time(&mut self) {
        self.event_loop.clock = Clock::Virtual(self.event_loop.now_ns());
    }

    /// Queue a task that calls `callback` with no arguments.
    pub fn post_task(&mut self, callback: Owned<E::Value>) {
        self.event_loop.tasks.push_back(Task::Call(Root::new(callback)));
    }

    pub fn event_loop(&self) -> &EventLoop<E> {
        &self.event_loop
    }

    /// Run tasks and due timers, each followed by a checkpoint, until
    /// none is ready or `max` have run; how many ran.
    pub fn run_tasks(&mut self, e: &mut E, max: usize) -> usize {
        let mut n = 0;
        while n < max && (self.recover(e) || e.halt() == Halt::Running) {
            if let Some(t) = self.event_loop.tasks.pop_front() {
                match t {
                    Task::Call(cb) => {
                        let r = e.call(self, cb.get(), E::undefined(), &[]);
                        self.finish_task(e, r);
                        cb.release(e);
                    }
                    Task::Script(t) => self.script_task(e, t),
                }
            } else if let Some(t) = self.event_loop.take_due() {
                self.run_timer(e, t);
            } else {
                break;
            }
            n += 1;
        }
        n
    }

    fn run_timer(&mut self, e: &mut E, t: Timer<E>) {
        if t.abort {
            self.event_loop.by_id.remove(&t.id);
            self.timed_out(e, t.handler.get());
            t.handler.release(e);
            // SAFETY: undefined carries no count
            self.finish_task(e, unsafe { Owned::from_raw(E::undefined()) });
            return;
        }
        let saved = std::mem::replace(&mut self.event_loop.nesting, t.nesting);
        let r = if e.is_callable(t.handler.get()) {
            let window = e.global(self.realm);
            let args: Vec<E::Value> = t.args.iter().map(Root::get).collect();
            let r = e.call(self, t.handler.get(), window.get(), &args);
            e.release(window);
            r
        } else {
            let src = e.as_string(t.handler.get()).unwrap_or_else(DomString::empty);
            let url = DomString::from_wtf8(b"timer");
            e.evaluate(self, self.realm, &src, &ScriptOrigin { url: &url, line: 1, column: 0 })
        };
        self.event_loop.nesting = saved;
        let stopped = self.finish_task(e, r);
        // cleared by its own callback when its entry is gone; one stopped could take every turn
        let live = self.event_loop.by_id.remove(&t.id).is_some();
        match t.repeat {
            Some(ms) if live && !stopped && e.halt() == Halt::Running => {
                // each run nests one deeper, so a fast interval is clamped
                // after the first few as nested timeouts are
                let t = Timer { nesting: t.nesting + 1, ..t };
                self.event_loop.schedule(t, u64::from(ms));
            }
            _ => {
                t.handler.release(e);
                for a in t.args {
                    a.release(e);
                }
            }
        }
    }

    /// Give back the timers and tasks: the window is going.
    pub(crate) fn close_loop(&mut self, e: &mut E) {
        for (_, t) in std::mem::take(&mut self.event_loop.timers) {
            t.handler.release(e);
            for a in t.args {
                a.release(e);
            }
        }
        self.event_loop.by_id.clear();
        for t in std::mem::take(&mut self.event_loop.tasks) {
            if let Task::Call(cb) = t {
                cb.release(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use madcawl_js::{Engine, Owned, Root};
    use madcawl_js_stub::Stub;

    use super::EventLoop;

    fn handler() -> Root<Stub> {
        // SAFETY: undefined carries no count
        Root::new(unsafe { Owned::from_raw(Stub::undefined()) })
    }

    #[test]
    fn timer_ids_wrap_past_live_ones() {
        let mut l = EventLoop::<Stub>::new();
        let kept = l.add_timer(handler(), Vec::new(), 1000, true);
        assert_eq!(kept, 1);
        l.next_id = i32::MAX as u32 - 1;
        let ids: Vec<u32> = (0..3).map(|_| l.add_timer(handler(), Vec::new(), 1000, false)).collect();
        assert_eq!(ids, [i32::MAX as u32 - 1, i32::MAX as u32, 2], "1 is still the interval's");
        assert_eq!(l.pending(), 4);
        assert!(l.remove_timer(1).is_some_and(|t| t.repeat == Some(1000)), "clearing 1 clears the interval");
    }
}
