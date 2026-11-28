//! The requests a document's script makes while it loads, an
//! XMLHttpRequest's and a worker's script, fetched on the page's
//! connection as its script sources are: in the agent session's cookie
//! partition, in the page's request log and in what a wait counts as in
//! flight. Each carries its method, headers and body, the document that
//! made it, its credentials mode and its mode, which the network service
//! applies at every hop (cookies, `Origin`, where it may go); the network
//! service drops the headers a requester may not set. Its response goes
//! back whole, whatever its status. A
//! document makes at most `MAX_SCRIPT_FETCHES`, each with a body of at
//! most `MAX_BODY` bytes; past that, one fails at once.

use std::collections::HashMap;

use madcawl_net::Url;
pub use madcawl_net::fetch::{Credentials, Mode};
use madcawl_net::wire::{Frame, kind, tag};

use super::loader::{Loader, Pending, Stage};
use super::net::NetConn;
use super::scripts::{Scripting, initiator};

/// The most requests one document's script makes.
pub const MAX_SCRIPT_FETCHES: usize = 1024;
/// The longest request body sent, in bytes: a frame carries it whole.
pub const MAX_BODY: usize = 8 << 20;
/// The initiator of a request made by a document whose origin is opaque.
const OPAQUE: &str = "about:blank";

/// A request a document's script made.
pub struct ScriptFetch {
    pub id: u64,
    /// `xhr` or `worker`, as the request log shows it
    pub what: &'static str,
    pub method: String,
    /// Absolute
    pub url: String,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Option<Vec<u8>>,
    /// The URL of the document that made it; `None` for a document whose
    /// origin is opaque
    pub initiator: Option<String>,
    pub credentials: Credentials,
    pub mode: Mode,
}

/// The response to a request a document's script made.
pub struct ScriptResponse {
    pub status: u16,
    pub status_text: String,
    /// The URL it ended at, after redirects
    pub url: String,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}

/// The requests of a document's script in flight: network request id
/// to the page's request id, and how many were asked for.
#[derive(Default)]
pub(crate) struct ScriptFetches {
    by_net: HashMap<u64, u64>,
    asked: usize,
}

impl Pending {
    /// What a request script made keeps of its response's head: the
    /// reason phrase and the headers.
    pub(super) fn keep_head(&mut self, head: &Frame) {
        if matches!(self.what, "xhr" | "worker") {
            self.reason = head.get_str(tag::REASON).unwrap_or("").to_owned();
            self.headers = head.headers();
        }
    }

    /// The response of a request that did not fail, of any status.
    pub fn into_response(self) -> Option<ScriptResponse> {
        let status = self.status.filter(|_| self.failed.is_none())?;
        let url = self.final_url.unwrap_or(self.url);
        Some(ScriptResponse { status, status_text: self.reason, url, headers: self.headers, body: self.body })
    }
}

impl Loader {
    /// Drops the requests of a document's script that ended once the
    /// scripts stage is over, when no script is left to take them.
    pub(crate) fn drop_script_responses(&mut self) {
        if !matches!(self.stage, Stage::Scripts(_)) {
            self.inflight.retain(|(_, p)| !(p.is_done() && matches!(p.what, "xhr" | "worker")));
        }
    }
}

impl Scripting {
    /// Asks for the requests the page's script made since the last call,
    /// and hands it the responses that have arrived.
    pub(crate) fn script_fetches(&mut self, conn: &mut NetConn, inflight: &mut Vec<(u64, Pending)>) -> std::io::Result<()> {
        for f in self.page.fetches() {
            let http = Url::parse(&f.url).is_ok_and(|u| matches!(u.scheme(), "http" | "https"));
            if !http || self.requests.asked >= MAX_SCRIPT_FETCHES || f.body.as_ref().is_some_and(|b| b.len() > MAX_BODY) {
                self.page.fetch_done(f.id, None);
                continue;
            }
            let credentials = match f.credentials {
                Credentials::Omit => 0,
                Credentials::Include => 1,
                Credentials::SameOrigin => 2,
            };
            let mode = match f.mode {
                Mode::NoCors => 0,
                Mode::Cors => 1,
                Mode::SameOrigin => 2,
            };
            let mut frame = Frame::new(kind::FETCH)
                .str(tag::METHOD, &f.method)
                .str(tag::URL, &f.url)
                .str(tag::INITIATOR, &f.initiator.as_deref().map_or_else(|| OPAQUE.to_owned(), initiator))
                .num(tag::CREDENTIALS, credentials)
                .num(tag::MODE, mode);
            for (n, v) in &f.headers {
                frame = frame.header(n, v);
            }
            if let Some(b) = &f.body {
                frame = frame.bytes(tag::BODY, b);
            }
            let net = conn.fetch(frame)?;
            inflight.push((net, Pending::new(f.what, &f.method, f.url)));
            self.requests.by_net.insert(net, f.id);
            self.requests.asked += 1;
        }
        let done: Vec<u64> = self.requests.by_net.keys().copied().filter(|n| inflight.iter().any(|(i, p)| i == n && p.is_done())).collect();
        for net in done {
            let id = self.requests.by_net.remove(&net).expect("a script request");
            let i = inflight.iter().position(|(x, _)| *x == net).expect("its request");
            let (_, p) = inflight.remove(i);
            self.page.fetch_done(id, p.into_response());
        }
        Ok(())
    }
}
