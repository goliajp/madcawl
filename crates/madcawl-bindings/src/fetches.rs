//! Requests script makes that the embedder fetches: an XMLHttpRequest's,
//! and a worker's script. The embedder takes the requests
//! ([`DomHost::take_fetches`]), fetches each as asked, in the page's
//! cookie partition, as a request of the document that made it (whose
//! origin and site its cookies and `Origin` are decided by), and hands
//! back the response, or `None` for a
//! network error ([`DomHost::fetch_done`]); a task then gives it to
//! whoever asked, if it still waits. At most `MAX_IN_FLIGHT` are asked
//! for or awaited at once in a host: past that a request fails in a
//! task, as a network error would.

use std::collections::HashMap;

use madcawl_js::{Engine, Root};

use crate::event_loop::Task;
use crate::host::DomHost;
use crate::side::SideRef;

/// Requests asked for or awaited at once in one host.
pub(crate) const MAX_IN_FLIGHT: usize = 64;

/// Whether cookies go with a request and are stored from its responses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Credentials {
    Omit,
    /// only to the initiator's origin, redirects included
    SameOrigin,
    Include,
}

/// The rules on where a request goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// to another origin only as a request that needs no preflight, with
    /// `Origin`; the requester checks the response against CORS
    Cors,
    /// only to the initiator's origin, redirects included
    SameOrigin,
}

/// A request for the embedder to fetch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchRequest {
    pub id: u64,
    /// What it is for: `xhr` or `worker`, as a request log shows it
    pub destination: &'static str,
    pub method: String,
    /// Absolute, HTTP(S)
    pub url: String,
    /// The request's own headers: none of the forbidden ones
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Option<Vec<u8>>,
    /// The URL of the document of the window that made it (for a
    /// document that takes its origin from its parent, the parent's);
    /// `None` when that window's origin is opaque
    pub initiator: Option<String>,
    pub credentials: Credentials,
    pub mode: Mode,
}

/// A response the embedder fetched, after its redirects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchResponse {
    pub status: u16,
    pub status_text: String,
    /// The URL it ended at
    pub url: String,
    /// The headers a page may see: no `Set-Cookie`
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}

/// Who waits on a request.
pub(crate) enum Owner<E: Engine> {
    /// An XMLHttpRequest of window `window`, kept by its wrapper while it waits
    Xhr { xhr: SideRef, wrapper: Root<E>, window: u64 },
    /// The script of a worker (`workers.rs`)
    Worker(u64),
}

pub(crate) struct Fetches<E: Engine> {
    pub requests: Vec<FetchRequest>,
    pub waiting: HashMap<u64, Owner<E>>,
    /// Responses whose task is queued.
    done: HashMap<u64, Option<FetchResponse>>,
    next: u64,
}

impl<E: Engine> Fetches<E> {
    pub(crate) fn new() -> Self {
        Fetches { requests: Vec::new(), waiting: HashMap::new(), done: HashMap::new(), next: 0 }
    }
}

impl<E: Engine> DomHost<E> {
    /// Ask for `req` (its id is set here) for `owner`; its id. Past
    /// `MAX_IN_FLIGHT`, or with `local` given (a response the host has
    /// itself, such as a blob URL's, or `None` for one that fails at
    /// once), nothing goes to the embedder and a task delivers it.
    pub(crate) fn start_fetch(&mut self, mut req: FetchRequest, owner: Owner<E>, local: Option<Option<FetchResponse>>) -> u64 {
        self.fetches.next += 1;
        let id = self.fetches.next;
        let full = self.fetches.waiting.len() >= MAX_IN_FLIGHT;
        self.fetches.waiting.insert(id, owner);
        match local {
            _ if full => self.fetch_done(id, None),
            Some(r) => self.fetch_done(id, r),
            None => {
                req.id = id;
                self.fetches.requests.push(req);
            }
        }
        id
    }

    /// Stop waiting on request `id`: what arrives for it is dropped.
    pub(crate) fn cancel_fetch(&mut self, e: &mut E, id: u64) {
        self.fetches.requests.retain(|r| r.id != id);
        self.fetches.done.remove(&id);
        if let Some(Owner::Xhr { wrapper, .. }) = self.fetches.waiting.remove(&id) {
            wrapper.release(e);
        }
    }

    /// The requests to fetch since the last call.
    pub fn take_fetches(&mut self) -> Vec<FetchRequest> {
        std::mem::take(&mut self.fetches.requests)
    }

    /// Request `id` ended with `response`, or a network error (`None`):
    /// a task gives it to whoever waits on it. One no one waits on any
    /// more is ignored.
    pub fn fetch_done(&mut self, id: u64, response: Option<FetchResponse>) {
        if !self.fetches.waiting.contains_key(&id) || self.fetches.done.contains_key(&id) {
            return;
        }
        self.fetches.done.insert(id, response);
        self.event_loop.tasks.push_back(Task::Fetched(id));
    }

    /// How many requests are asked for or awaited.
    pub fn fetches_in_flight(&self) -> usize {
        self.fetches.waiting.len()
    }

    /// The task of a request that ended.
    pub(crate) fn fetch_task(&mut self, e: &mut E, id: u64) {
        let Some(response) = self.fetches.done.remove(&id) else { return };
        match self.fetches.waiting.remove(&id) {
            Some(Owner::Xhr { xhr, wrapper, .. }) => {
                self.xhr_response(e, xhr, id, response);
                wrapper.release(e);
            }
            Some(Owner::Worker(w)) => self.worker_script(e, w, response),
            None => {}
        }
    }

    /// Window `w` is gone: its requests are not waited on, and its
    /// XMLHttpRequests wait on nothing, with no timer.
    pub(crate) fn drop_window_fetches(&mut self, e: &mut E, w: u64) {
        let ids: Vec<(u64, SideRef)> = self
            .fetches
            .waiting
            .iter()
            .filter_map(|(&id, o)| match o {
                Owner::Xhr { xhr, window, .. } if *window == w => Some((id, *xhr)),
                _ => None,
            })
            .collect();
        for (id, xhr) in ids {
            self.xhr_clear_timer(e, xhr);
            self.xhr_mut(xhr).request = None;
            self.cancel_fetch(e, id);
        }
    }

    pub(crate) fn close_fetches(&mut self, e: &mut E) {
        let ids: Vec<u64> = self.fetches.waiting.keys().copied().collect();
        for id in ids {
            self.cancel_fetch(e, id);
        }
    }
}
