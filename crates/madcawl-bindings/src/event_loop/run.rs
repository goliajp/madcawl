//! Running a due timer, script's or the host's own, and giving back
//! the timers and tasks when the window goes.

use madcawl_js::{Engine, Halt, Owned, Root, ScriptOrigin};
use madcawl_str::DomString;

use super::{Task, Timer, TimerKind};
use crate::host::DomHost;

impl<E: Engine> DomHost<E> {
    pub(super) fn run_timer(&mut self, e: &mut E, t: Timer<E>) {
        if t.kind != TimerKind::Script {
            self.event_loop.by_id.remove(&t.id);
            match t.kind {
                TimerKind::XhrTimeout(id) => self.xhr_timer_fired(e, t.handler.get(), id),
                _ => self.timed_out(e, t.handler.get()),
            }
            t.handler.release(e);
            // SAFETY: undefined carries no count
            self.finish_task(e, unsafe { Owned::from_raw(E::undefined()) });
            return;
        }
        let realm = self.realm_of(t.window).expect("the timers of a window go with it");
        let saved = std::mem::replace(&mut self.event_loop.nesting, t.nesting);
        let worker = self.workers.live.contains_key(&t.window).then_some(t.window);
        let current = worker.or(self.workers.current);
        let saved_worker = std::mem::replace(&mut self.workers.current, current);
        let r = if e.is_callable(t.handler.get()) {
            let window = e.global(realm);
            let args: Vec<E::Value> = t.args.iter().map(Root::get).collect();
            let r = e.call(self, t.handler.get(), window.get(), &args);
            e.release(window);
            r
        } else {
            let src = e.as_string(t.handler.get()).unwrap_or_else(DomString::empty);
            let url = DomString::from_wtf8(b"timer");
            e.evaluate(self, realm, &src, &ScriptOrigin { url: &url, line: 1, column: 0 })
        };
        self.event_loop.nesting = saved;
        let stopped = self.finish_task(e, r);
        self.workers.current = saved_worker;
        // cleared by its own callback when its entry is gone; one stopped
        // could take every turn; one whose window went with its run is done
        let live = self.event_loop.by_id.remove(&t.id).is_some() && self.realm_of(t.window).is_some();
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
            match t {
                Task::Call(cb) => cb.release(e),
                Task::Settle(_, resolve, value) => {
                    resolve.release(e);
                    value.release(e);
                }
                Task::Script(_) | Task::Fetched(_) => {}
                Task::Worker(t) => t.release(e),
            }
        }
    }
}
