//! Dedicated workers. A worker runs in a realm of its own in this host's
//! isolate, on the window's thread, and its tasks go on the window's
//! event loop: the engine and every count the host keeps live on the one
//! thread script runs on, and a second thread would need an isolate and a
//! host of its own. What a worker and its window see of each other is
//! what dedicated workers show: values go across by structured clone
//! into the other realm, delivered in a task, never shared. A worker is
//! not parallel to its window, so one that runs long holds the window
//! up as the window's own script would, under the same time limit.
//!
//! A worker gets an id from the windows' id space (`frames.rs`), which
//! its global's listeners, timers and blob URLs are filed under. Its
//! script is a blob URL's bytes, read as the worker is made (so revoking
//! the URL then is safe), or fetched by the embedder from the window's
//! origin (`fetches.rs`). Messages to it wait until its script has run.
//! It lives until `terminate()`, its own `close()` or its window going,
//! holding its Worker object and that object's listeners: an unreachable
//! worker is not collected. A window has at most `MAX_PER_WINDOW` at once;
//! a worker made past that never starts, and `error` fires at it.

use std::collections::HashMap;

use alt_url::{Origin, Url};
use madcawl_js::{Engine, Root};
use madcawl_str::{DomString, atom};

use crate::event::Target;
use crate::event_kinds::ErrorPlace;
use crate::event_loop::Task;
use crate::host::DomHost;
use crate::listeners::Listener;
use crate::side::SideRef;

/// Workers one window may have at once.
pub(crate) const MAX_PER_WINDOW: usize = 16;

pub(crate) struct Worker<E: Engine> {
    /// The window that made it
    pub owner: u64,
    pub object: SideRef,
    /// The Worker object's wrapper, held while the worker lives
    pub wrapper: Root<E>,
    pub realm: E::Realm,
    pub url: Url,
    pub origin: Origin,
    pub listeners: Vec<Listener<E>>,
    /// Its script ran: messages to it are delivered
    pub started: bool,
    /// Messages to it, made in its realm, until it starts
    pub inbox: Vec<Root<E>>,
    /// It called `close()`: nothing more is delivered to it
    pub closing: bool,
    /// The fetch of its script, while it waits
    pub request: Option<u64>,
}

#[derive(Default)]
pub(crate) struct Workers<E: Engine> {
    pub live: HashMap<u64, Worker<E>>,
    /// The worker whose script runs, for an exception nothing caught
    pub current: Option<u64>,
}

/// A worker's tasks.
pub(crate) enum WorkerTask<E: Engine> {
    /// Run its script
    Run(u64, DomString),
    /// A message for its global, made in its realm
    ToWorker(u64, Root<E>),
    /// A message for its Worker object, made in the realm of window
    /// `owner`, and the object's wrapper: delivered after the worker
    /// closed itself too, while its window lives
    ToObject { worker: u64, owner: u64, object: SideRef, wrapper: Root<E>, data: Root<E> },
    /// An error its global did not cancel, for its Worker object
    Error(u64, Box<ErrorPlace>),
    /// Its script could not be had: `error` at its Worker object, of
    /// window `owner`, held by its wrapper until then
    Failed { owner: u64, object: SideRef, wrapper: Root<E> },
    /// It closed itself
    Close(u64),
}

impl<E: Engine> WorkerTask<E> {
    pub(crate) fn release(self, e: &mut E) {
        match self {
            WorkerTask::ToWorker(_, v) | WorkerTask::Failed { wrapper: v, .. } => v.release(e),
            WorkerTask::ToObject { wrapper, data, .. } => {
                wrapper.release(e);
                data.release(e);
            }
            _ => {}
        }
    }

    fn worker(&self) -> Option<u64> {
        match self {
            WorkerTask::Run(w, _) | WorkerTask::ToWorker(w, _) | WorkerTask::ToObject { worker: w, .. } | WorkerTask::Error(w, _) | WorkerTask::Close(w) => Some(*w),
            WorkerTask::Failed { .. } => None,
        }
    }

    /// The window whose Worker object it is for, when it outlives the worker.
    fn owner(&self) -> Option<u64> {
        match self {
            WorkerTask::ToObject { owner, .. } | WorkerTask::Failed { owner, .. } => Some(*owner),
            _ => None,
        }
    }
}

impl<E: Engine> DomHost<E> {
    pub(crate) fn queue_worker(&mut self, t: WorkerTask<E>) {
        self.event_loop.tasks.push_back(Task::Worker(t));
    }

    /// How many workers window `w` has.
    pub(crate) fn workers_of(&self, w: u64) -> usize {
        self.workers.live.values().filter(|k| k.owner == w).count()
    }

    /// How many workers are alive.
    pub fn worker_count(&self) -> usize {
        self.workers.live.len()
    }

    /// The script of worker `w` came, or could not be had: one that
    /// ended at another origin after redirects is not run.
    pub(crate) fn worker_script(&mut self, e: &mut E, w: u64, response: Option<crate::fetches::FetchResponse>) {
        let Some(k) = self.workers.live.get_mut(&w) else { return };
        k.request = None;
        let same = |r: &crate::fetches::FetchResponse| Url::parse(&r.url).is_ok_and(|u| crate::frames::same_origin(&u.origin(), &k.origin));
        match response.filter(|r| (200..300).contains(&r.status) && same(r)) {
            Some(r) => self.queue_worker(WorkerTask::Run(w, DomString::from_wtf8(String::from_utf8_lossy(&r.body).as_bytes()))),
            None => {
                let (owner, object, wrapper) = (k.owner, k.object, Root::new(e.retain(k.wrapper.get())));
                self.queue_worker(WorkerTask::Failed { owner, object, wrapper });
            }
        }
    }

    pub(crate) fn worker_task(&mut self, e: &mut E, t: WorkerTask<E>) {
        match t {
            WorkerTask::Run(w, src) => self.run_worker(e, w, &src),
            WorkerTask::ToWorker(w, v) => {
                if self.workers.live.get(&w).is_some_and(|k| !k.closing) {
                    let saved = self.workers.current.replace(w);
                    self.deliver_message(e, Target::Window(w), v.get());
                    self.workers.current = saved;
                }
                v.release(e);
            }
            WorkerTask::ToObject { object, wrapper, data, .. } => {
                self.deliver_message(e, Target::Side(object), data.get());
                wrapper.release(e);
                data.release(e);
            }
            WorkerTask::Error(w, place) => self.worker_error_at_object(e, w, *place),
            WorkerTask::Failed { object, wrapper, .. } => {
                self.fire_simple(e, Target::Side(object), atom!("error"), false);
                wrapper.release(e);
            }
            WorkerTask::Close(w) => self.end_worker(e, w, false),
        }
    }

    /// Run worker `w`'s script, then let in the messages that waited.
    fn run_worker(&mut self, e: &mut E, w: u64, src: &DomString) {
        let Some(k) = self.workers.live.get(&w) else { return };
        let (realm, url) = (k.realm, DomString::from_wtf8(k.url.as_str().as_bytes()));
        let saved = self.workers.current.replace(w);
        let r = e.evaluate(self, realm, src, &madcawl_js::ScriptOrigin { url: &url, line: 1, column: 0 });
        self.finish_task(e, r);
        self.workers.current = saved;
        let Some(k) = self.workers.live.get_mut(&w) else { return };
        k.started = true;
        for v in std::mem::take(&mut k.inbox) {
            self.queue_worker(WorkerTask::ToWorker(w, v));
        }
    }

    /// Worker `w` goes: its realm detached, its timers, listeners,
    /// messages and blob URLs dropped, its Worker object let go of, and
    /// its tasks, those of messages to its object too.
    pub(crate) fn destroy_worker(&mut self, e: &mut E, w: u64) {
        self.end_worker(e, w, true);
    }

    /// [`DomHost::destroy_worker`]; without `purge`, as when it closed
    /// itself, the messages it posted are still delivered.
    fn end_worker(&mut self, e: &mut E, w: u64, purge: bool) {
        let Some(k) = self.workers.live.remove(&w) else { return };
        if purge {
            let tasks = std::mem::take(&mut self.event_loop.tasks);
            for t in tasks {
                match t {
                    Task::Worker(t) if t.worker() == Some(w) => t.release(e),
                    t => self.event_loop.tasks.push_back(t),
                }
            }
        }
        if let Some(id) = k.request {
            self.cancel_fetch(e, id);
        }
        for mut l in k.listeners {
            l.callback.clear(e);
        }
        for v in k.inbox {
            v.release(e);
        }
        for t in self.event_loop.remove_window_timers(w) {
            t.handler.release(e);
            t.args.into_iter().for_each(|a| a.release(e));
        }
        self.blob_urls.drop_window(w);
        k.wrapper.release(e);
        e.detach_realm(k.realm);
    }

    /// The workers of window `w` go with it, and what was queued for
    /// its Worker objects.
    pub(crate) fn drop_window_workers(&mut self, e: &mut E, w: u64) {
        let ids: Vec<u64> = self.workers.live.iter().filter(|(_, k)| k.owner == w).map(|(&id, _)| id).collect();
        for id in ids {
            self.destroy_worker(e, id);
        }
        for t in std::mem::take(&mut self.event_loop.tasks) {
            match t {
                Task::Worker(t) if t.owner() == Some(w) => t.release(e),
                t => self.event_loop.tasks.push_back(t),
            }
        }
    }

    pub(crate) fn close_workers(&mut self, e: &mut E) {
        let ids: Vec<u64> = self.workers.live.keys().copied().collect();
        for id in ids {
            self.destroy_worker(e, id);
        }
    }
}
