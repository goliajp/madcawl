//! The network process's service: content processes connect over a Unix
//! socket and send frames (`madcawl_net::wire`); each fetch runs on its own
//! thread, at most `in_flight` per connection, `requests` in all and
//! `clients` connections at a time. When a connection closes, its fetches
//! are cancelled and their sockets shut down. A fetch that is still
//! waiting for a pool slot, a DNS answer or a connect is not woken: it
//! holds its request slot until that step's own limit (connect timeout,
//! DNS timeout, bounded by the request deadline) and then sees the
//! cancellation.
//!
//! Memory: a response body goes out as it is decoded, with at most
//! `BODY_CHUNK` bytes of it waiting per request. A request body is held
//! (once) for as long as its fetch runs, and all of them together stay
//! within `body_bytes`; a fetch over that budget is refused. Reading a
//! frame takes up to `MAX_FRAME` bytes per connection, one frame at a time.
//!
//! Sockets, TLS state and the cookie jar stay here: a response's
//! `Set-Cookie` fields are stripped before it goes out, and script access
//! to cookies never reads or writes `HttpOnly` ones.

mod fetch;

use fetch::run_fetch;
use madcawl_net::cookie::{Api, SiteContext, serialize};
use madcawl_net::fetch::now_secs;
use madcawl_net::sock::Cancel;
use madcawl_net::wire::{Frame, kind, tag};
use madcawl_net::{Client, Url};
use std::io;
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

const EMFILE: i32 = 24;
const ENFILE: i32 = 23;
/// out of descriptors the pending connection stays queued, so accepting
/// again at once would spin
const ACCEPT_BACKOFF: Duration = Duration::from_millis(50);

/// Requests are shared out so that no client can lock the others out:
/// each client may always run `reserve()` requests, and runs more only by
/// drawing on a shared pool of what is left of `requests` once every
/// possible client's reserve is set aside. A client that holds up to
/// `in_flight` requests leaves the others their reserves; a cap of
/// `clients × in_flight` in all would not be needed for that, and would
/// allow that many threads.
#[derive(Clone, Copy, Debug)]
pub struct ServerLimits {
    pub clients: usize,
    /// requests at a time from one client
    pub in_flight: usize,
    /// requests at a time from all clients together
    pub requests: usize,
    /// bytes of request bodies held at a time by all clients together
    pub body_bytes: usize,
    /// how long a write to a client may block before the client is dropped
    pub write_timeout: Duration,
}

impl Default for ServerLimits {
    fn default() -> Self {
        ServerLimits {
            clients: 64,
            in_flight: 32,
            requests: 256,
            body_bytes: 64 << 20,
            write_timeout: Duration::from_secs(30),
        }
    }
}

impl ServerLimits {
    /// Requests each client may run without drawing on the shared pool;
    /// all reserves together are at most half of `requests`.
    pub fn reserve(&self) -> usize {
        self.requests / (2 * self.clients.max(1))
    }

    fn shared(&self) -> usize {
        self.requests - self.clients * self.reserve()
    }
}

/// Response headers a content process never sees.
pub fn is_hidden_response_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("set-cookie") || name.eq_ignore_ascii_case("set-cookie2")
}

/// `n` units under a cap, given back when dropped.
struct Slot(Arc<AtomicUsize>, usize);

impl Slot {
    fn take(count: &Arc<AtomicUsize>, n: usize, cap: usize) -> Option<Slot> {
        if count.fetch_add(n, Ordering::SeqCst).saturating_add(n) > cap {
            count.fetch_sub(n, Ordering::SeqCst);
            return None;
        }
        Some(Slot(count.clone(), n))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(self.1, Ordering::SeqCst);
    }
}

/// The counters all clients share.
#[derive(Clone, Default)]
struct Shared {
    /// requests drawn from the shared pool
    requests: Arc<AtomicUsize>,
    body_bytes: Arc<AtomicUsize>,
}

/// What one running request holds: its place among its client's, a place
/// in the shared pool unless it is within the client's reserve, and its
/// body's bytes.
struct Held {
    _mine: Slot,
    _shared: Option<Slot>,
    _body: Slot,
}

fn admit(mine: &Arc<AtomicUsize>, shared: &Shared, limits: &ServerLimits, body: usize) -> Result<Held, &'static str> {
    const BUSY: &str = "too many requests in flight";
    // only this client's reader adds to `mine`, so it cannot grow meanwhile
    let within_reserve = mine.load(Ordering::SeqCst) < limits.reserve();
    let _mine = Slot::take(mine, 1, limits.in_flight).ok_or(BUSY)?;
    let _shared = match within_reserve {
        true => None,
        false => Some(Slot::take(&shared.requests, 1, limits.shared()).ok_or(BUSY)?),
    };
    let _body = Slot::take(&shared.body_bytes, body, limits.body_bytes).ok_or("request bodies over their memory budget")?;
    Ok(Held { _mine, _shared, _body })
}

/// Cancels a client's fetches when its connection is done with.
struct CancelOnDrop(Arc<Cancel>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Serves `listener` for good; one thread per connected client.
pub fn serve(listener: UnixListener, client: Arc<Client>, limits: ServerLimits) {
    serve_streams(listener.incoming(), client, limits);
}

/// Serves each connection `streams` yields until it ends. A failed accept
/// is logged and the next one tried.
pub fn serve_streams(streams: impl IntoIterator<Item = io::Result<UnixStream>>, client: Arc<Client>, limits: ServerLimits) {
    let live = Arc::new(AtomicUsize::new(0));
    let shared = Shared::default();
    for stream in streams {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("madcawl-netd: accept: {e}");
                if matches!(e.raw_os_error(), Some(EMFILE | ENFILE)) {
                    std::thread::sleep(ACCEPT_BACKOFF);
                }
                continue;
            }
        };
        let Some(slot) = Slot::take(&live, 1, limits.clients) else { continue };
        if let Err(e) = stream.set_write_timeout(Some(limits.write_timeout)) {
            eprintln!("madcawl-netd: client socket: {e}");
            continue;
        }
        let (client, shared) = (client.clone(), shared.clone());
        let spawned = std::thread::Builder::new().name("netd-client".into()).spawn(move || {
            let _slot = slot;
            let _ = serve_client(stream, &client, &shared, limits);
        });
        if let Err(e) = spawned {
            eprintln!("madcawl-netd: client thread: {e}");
        }
    }
}

pub(crate) type Writer = Arc<Mutex<UnixStream>>;

/// Sends one frame; a failed or timed-out write leaves a partial frame on
/// the stream, so the connection is shut down.
pub(crate) fn send(w: &Writer, f: &Frame) -> io::Result<()> {
    let mut s = w.lock().unwrap_or_else(PoisonError::into_inner);
    let r = f.write_to(&mut *s);
    if r.is_err() {
        let _ = s.shutdown(Shutdown::Both);
    }
    r
}

pub(crate) fn error(id: u64, message: &str) -> Frame {
    Frame::new(kind::ERROR).num(tag::ID, id).str(tag::MESSAGE, message)
}

fn serve_client(stream: UnixStream, client: &Arc<Client>, shared: &Shared, limits: ServerLimits) -> io::Result<()> {
    let mut reader = stream.try_clone()?;
    let writer: Writer = Arc::new(Mutex::new(stream));
    let mine = Arc::new(AtomicUsize::new(0));
    // a client that stops sending, by closing or by a bad frame, is gone
    let cancel = CancelOnDrop(Cancel::new());
    while let Some(frame) = Frame::read_from(&mut reader)? {
        let id = frame.get_num(tag::ID).unwrap_or(0);
        match frame.kind {
            kind::FETCH => {
                let body = frame.field(tag::BODY).map_or(0, <[u8]>::len);
                let held = match admit(&mine, shared, &limits, body) {
                    Ok(h) => h,
                    Err(why) => {
                        send(&writer, &error(id, why))?;
                        continue;
                    }
                };
                let (client, w, c) = (client.clone(), writer.clone(), cancel.0.clone());
                let spawned = std::thread::Builder::new().name("netd-fetch".into()).spawn(move || {
                    let _held = held;
                    let _ = run_fetch(&client, &w, id, frame, c);
                });
                if let Err(e) = spawned {
                    send(&writer, &error(id, &format!("cannot start the request: {e}")))?;
                }
            }
            kind::COOKIE_GET | kind::COOKIE_SET => send(&writer, &cookies(client, id, &frame))?,
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn parse_url(frame: &Frame, t: u8) -> Option<Url> {
    Url::parse(frame.get_str(t)?).ok()
}

/// `document.cookie` for the document at URL: reading gives the cookies
/// script may see, writing stores one `Set-Cookie`-style line as script.
fn cookies(client: &Client, id: u64, frame: &Frame) -> Frame {
    let Some(url) = parse_url(frame, tag::URL) else { return error(id, "missing or bad url") };
    let mut jar = client.lock_jar();
    let reply = Frame::new(kind::COOKIES).num(tag::ID, id);
    if frame.kind == kind::COOKIE_SET {
        let line = frame.get_str(tag::TEXT).unwrap_or("");
        let ok = jar.set(&url, line, Api::Script, now_secs());
        return reply.num(tag::OK, u64::from(ok));
    }
    let list = jar.matching(&url, SiteContext::SameSite, Api::Script, now_secs());
    reply.str(tag::TEXT, &serialize(&list))
}
