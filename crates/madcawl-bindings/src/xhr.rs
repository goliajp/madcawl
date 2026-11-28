//! An XMLHttpRequest's request and response, asynchronous only: `send`
//! asks the embedder for the request (`fetches.rs`), and the response
//! comes back whole, so `readystatechange` (HEADERS_RECEIVED, then
//! LOADING when there is a body), one `progress`, then DONE, `load` and
//! `loadend` fire in one task. Every request is in CORS mode, as its
//! window's document's: one to another origin, the first hop or after a
//! redirect, goes only as a simple request (a preflight is not done yet,
//! so one that would need it fails as a network error; the network
//! service refuses it too) and is read only past the CORS check. Cookies
//! go to another origin only when `withCredentials` is set; otherwise
//! the credentials mode is same-origin, so no hop to another origin
//! carries or stores any. A blob URL of the window's origin is read from
//! the blob URL store. The request waits at most `timeout` milliseconds
//! from `send`, on the event loop's clock, on one timer that goes with
//! its window; an XMLHttpRequest that waits is kept alive by the request.

use std::rc::Rc;

use alt_url::Url;
use madcawl_js::{Engine, Root};
use madcawl_str::{Atom, atom};

use crate::fetches::{Credentials, FetchRequest, FetchResponse, Mode, Owner};
use crate::frames::same_origin;
use crate::host::DomHost;
use crate::side::{SideData, SideRef};
use crate::xhr_http::{cors_allows, is_safelisted_request_header, visible};

pub(crate) const UNSENT: u16 = 0;
pub(crate) const OPENED: u16 = 1;
pub(crate) const HEADERS_RECEIVED: u16 = 2;
pub(crate) const LOADING: u16 = 3;
pub(crate) const DONE: u16 = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Default,
    ArrayBuffer,
    Blob,
    Document,
    Json,
    Text,
}

pub(crate) struct Response {
    pub status: u16,
    pub status_text: Vec<u8>,
    pub url: String,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Rc<[u8]>,
}

pub(crate) struct XhrState {
    pub state: u16,
    pub send: bool,
    pub method: String,
    pub url: Option<Url>,
    pub headers: Vec<(String, Vec<u8>)>,
    pub timeout: u32,
    pub credentials: bool,
    pub kind: Kind,
    pub mime: Option<String>,
    /// The request waited on, while it is.
    pub request: Option<u64>,
    /// The timer of its timeout, while the request waits: one at a time.
    pub timer: Option<u32>,
    /// When `send` was called, on the event loop's clock.
    pub sent_ns: u64,
    pub response: Option<Response>,
    /// Bumped by `open` and `abort`: what fires for an older request stops.
    pub generation: u64,
    /// The window it was made in.
    pub window: u64,
}

impl XhrState {
    pub(crate) fn new(window: u64) -> Self {
        XhrState {
            state: UNSENT, send: false, method: String::new(), url: None, headers: Vec::new(), timeout: 0, credentials: false,
            kind: Kind::Default, mime: None, request: None, timer: None, sent_ns: 0, response: None, generation: 0, window,
        }
    }
}

impl<E: Engine> DomHost<E> {
    pub(crate) fn xhr(&self, r: SideRef) -> &XhrState {
        match &self.sides.get(r).data {
            SideData::Xhr(x) => x,
            _ => unreachable!("an XMLHttpRequest's side object"),
        }
    }

    pub(crate) fn xhr_mut(&mut self, r: SideRef) -> &mut XhrState {
        match &mut self.sides.get_mut(r).data {
            SideData::Xhr(x) => x,
            _ => unreachable!("an XMLHttpRequest's side object"),
        }
    }

    /// Stop waiting on `r`'s request and drop its response object; a new
    /// generation, so what an older one still fires stops.
    pub(crate) fn xhr_reset(&mut self, e: &mut E, r: SideRef) {
        self.xhr_clear_timer(e, r);
        if let Some(id) = self.xhr_mut(r).request.take() {
            self.cancel_fetch(e, id);
        }
        if let Some(mut m) = self.details.remove(&r) {
            m.clear(e);
        }
        let x = self.xhr_mut(r);
        x.generation += 1;
        x.response = None;
    }

    fn fire_xhr(&mut self, e: &mut E, r: SideRef, ty: Atom, generation: u64) -> bool {
        self.fire_simple(e, crate::event::Target::Side(r), ty, false);
        self.xhr(r).generation == generation
    }

    fn progress_xhr(&mut self, e: &mut E, r: SideRef, ty: Atom, n: u64, generation: u64) -> bool {
        self.fire_progress(e, r, ty, n, (n != 0).then_some(n));
        self.xhr(r).generation == generation
    }

    /// The standard's request error steps, with event `ty`: the request
    /// is done with a network error.
    pub(crate) fn xhr_error(&mut self, e: &mut E, r: SideRef, ty: Atom) {
        self.xhr_reset(e, r);
        let x = self.xhr_mut(r);
        (x.state, x.send) = (DONE, false);
        let g = x.generation;
        let _ = self.fire_xhr(e, r, atom!("readystatechange"), g) && self.progress_xhr(e, r, ty, 0, g) && self.progress_xhr(e, r, atom!("loadend"), 0, g);
    }

    /// Ask for `r`'s request, with `body` and its type, as `send` does
    /// once its checks passed.
    pub(crate) fn xhr_start(&mut self, e: &mut E, r: SideRef, body: Option<Vec<u8>>, ty: Option<String>) {
        let x = self.xhr(r);
        let (url, w) = (x.url.clone().expect("an opened request"), x.window);
        let mut headers = x.headers.clone();
        if let Some(t) = ty.filter(|_| !headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("content-type"))) {
            headers.push(("Content-Type".to_owned(), t.into_bytes()));
        }
        let cors = !same_origin(&url.origin(), &self.origin_of(w));
        let credentials = if x.credentials { Credentials::Include } else { Credentials::SameOrigin };
        let (method, initiator) = (x.method.clone(), self.initiator_of(w));
        let req = FetchRequest { id: 0, destination: "xhr", method, url: url.as_str().to_owned(), headers, body, initiator, credentials, mode: Mode::Cors };
        let local = self.local_response(&req, &url, cors);
        let wrapper = Root::new(self.wrap_side(e, r));
        let id = self.start_fetch(req, Owner::Xhr { xhr: r, wrapper, window: w }, local);
        let now = self.event_loop.now_ns();
        let x = self.xhr_mut(r);
        (x.request, x.send, x.sent_ns) = (Some(id), true, now);
        let (g, timeout) = (x.generation, x.timeout);
        if self.progress_xhr(e, r, atom!("loadstart"), 0, g) && timeout != 0 {
            self.xhr_timer(e, r, u64::from(timeout));
        }
    }

    /// What a request is answered with without the network: `Some(None)`
    /// for one that fails at once (not HTTP(S), a CORS request that
    /// needs a preflight), a blob URL's bytes, or `None` to fetch it.
    fn local_response(&self, req: &FetchRequest, url: &Url, cors: bool) -> Option<Option<FetchResponse>> {
        if url.scheme() == "blob" {
            let blob = self.blob_urls.get(url.as_str()).filter(|_| !cors && req.method == "GET");
            return Some(blob.map(|b| FetchResponse {
                status: 200,
                status_text: "OK".to_owned(),
                url: url.as_str().to_owned(),
                headers: vec![("Content-Type".to_owned(), b.ty.clone().into_bytes()), ("Content-Length".to_owned(), b.bytes().len().to_string().into_bytes())],
                body: b.bytes().to_vec(),
            }));
        }
        let simple = matches!(req.method.as_str(), "GET" | "HEAD" | "POST") && req.headers.iter().all(|(n, v)| is_safelisted_request_header(n, v));
        (!matches!(url.scheme(), "http" | "https") || (cors && !simple)).then_some(None)
    }

    /// The timer that times `r`'s request out `ms` from now, in place of
    /// the one it had, filed under its window so that it goes with it.
    pub(crate) fn xhr_timer(&mut self, e: &mut E, r: SideRef, ms: u64) {
        self.xhr_clear_timer(e, r);
        let x = self.xhr(r);
        let (Some(id), w) = (x.request, x.window) else { return };
        let wrapper = Root::new(self.wrap_side(e, r));
        let t = self.event_loop.add_host_timer(wrapper, ms, crate::event_loop::TimerKind::XhrTimeout(id), w);
        self.xhr_mut(r).timer = Some(t);
    }

    pub(crate) fn xhr_clear_timer(&mut self, e: &mut E, r: SideRef) {
        if let Some(t) = self.xhr_mut(r).timer.take().and_then(|t| self.event_loop.remove_timer(t)) {
            t.handler.release(e);
        }
    }

    /// The timer of request `id` fired: `xhr` is its XMLHttpRequest's
    /// wrapper, of a window that lives (its timers go with it). It
    /// times the request out, or waits again when the timeout grew.
    pub(crate) fn xhr_timer_fired(&mut self, e: &mut E, xhr: E::Value, id: u64) {
        let obj = e.host_of(xhr, &crate::generated::XMLHttpRequestClass::<E>::DEF).expect("an XMLHttpRequest's wrapper");
        let r = self.sides.of(obj).expect("a live XMLHttpRequest");
        self.xhr_mut(r).timer = None;
        let x = self.xhr(r);
        if x.request != Some(id) || x.timeout == 0 {
            return;
        }
        let due = x.sent_ns.saturating_add(u64::from(x.timeout) * 1_000_000);
        let now = self.event_loop.now_ns();
        if due > now {
            return self.xhr_timer(e, r, (due - now).div_ceil(1_000_000));
        }
        let ty = self.names.hold(b"timeout");
        self.xhr_error(e, r, ty);
    }

    /// Request `id` of `r` ended with `response`, or a network error.
    pub(crate) fn xhr_response(&mut self, e: &mut E, r: SideRef, id: u64, response: Option<FetchResponse>) {
        if self.xhr(r).request != Some(id) {
            return;
        }
        let x = self.xhr(r);
        let origin = self.origin_of(x.window);
        let checked = response.and_then(|resp| {
            let cors = Url::parse(&resp.url).map_or(true, |u| !same_origin(&u.origin(), &origin));
            let credentials = x.credentials;
            (!cors || cors_allows(&resp.headers, &origin.ascii_serialization(), credentials)).then(|| Response {
                status: resp.status,
                status_text: resp.status_text.into_bytes(),
                url: resp.url,
                headers: visible(resp.headers, cors, credentials),
                body: resp.body.into(),
            })
        });
        let Some(resp) = checked else { return self.xhr_error(e, r, atom!("error")) };
        self.xhr_clear_timer(e, r);
        self.xhr_mut(r).request = None;
        self.xhr_deliver(e, r, resp);
    }

    /// The response's events, stopping when a listener opens or aborts
    /// the request.
    fn xhr_deliver(&mut self, e: &mut E, r: SideRef, resp: Response) {
        let n = resp.body.len() as u64;
        let x = self.xhr_mut(r);
        x.response = Some(resp);
        x.state = HEADERS_RECEIVED;
        let g = x.generation;
        if !self.fire_xhr(e, r, atom!("readystatechange"), g) {
            return;
        }
        if n != 0 {
            self.xhr_mut(r).state = LOADING;
            if !self.fire_xhr(e, r, atom!("readystatechange"), g) {
                return;
            }
        }
        if !self.progress_xhr(e, r, atom!("progress"), n, g) {
            return;
        }
        let x = self.xhr_mut(r);
        (x.state, x.send) = (DONE, false);
        let _ = self.fire_xhr(e, r, atom!("readystatechange"), g) && self.progress_xhr(e, r, atom!("load"), n, g) && self.progress_xhr(e, r, atom!("loadend"), n, g);
    }
}
