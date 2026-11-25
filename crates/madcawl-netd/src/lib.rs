//! The network process's service: content processes connect over a Unix
//! socket and send frames (`madcawl_net::wire`); each fetch runs on its own
//! thread, at most `in_flight` per connection, `requests` in all and
//! `clients` connections at a time. A response body goes out as it is
//! decoded, at most `BODY_CHUNK` bytes held per request. Sockets, TLS state and the cookie jar stay here: a response's
//! `Set-Cookie` fields are stripped before it goes out, and script access
//! to cookies never reads or writes `HttpOnly` ones.

use madcawl_net::cookie::{Api, SiteContext, serialize};
use madcawl_net::fetch::{Redirect, Request, Sink, is_forbidden_method, now_secs};
use madcawl_net::wire::{BODY_CHUNK, Frame, kind, tag};
use madcawl_net::{Client, NetError, Response, Url};
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

#[derive(Clone, Copy, Debug)]
pub struct ServerLimits {
    pub clients: usize,
    /// requests at a time from one client
    pub in_flight: usize,
    /// requests at a time from all clients together
    pub requests: usize,
    /// how long a write to a client may block before the client is dropped
    pub write_timeout: Duration,
}

impl Default for ServerLimits {
    fn default() -> Self {
        ServerLimits { clients: 64, in_flight: 32, requests: 256, write_timeout: Duration::from_secs(30) }
    }
}

/// Response headers a content process never sees.
pub fn is_hidden_response_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("set-cookie") || name.eq_ignore_ascii_case("set-cookie2")
}

/// One place under a cap, given back when dropped.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(count: &Arc<AtomicUsize>, cap: usize) -> Option<Slot> {
        if count.fetch_add(1, Ordering::SeqCst) >= cap {
            count.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Slot(count.clone()))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
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
    let requests = Arc::new(AtomicUsize::new(0));
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
        let Some(slot) = Slot::take(&live, limits.clients) else { continue };
        if let Err(e) = stream.set_write_timeout(Some(limits.write_timeout)) {
            eprintln!("madcawl-netd: client socket: {e}");
            continue;
        }
        let (client, requests) = (client.clone(), requests.clone());
        let spawned = std::thread::Builder::new().name("netd-client".into()).spawn(move || {
            let _slot = slot;
            let _ = serve_client(stream, &client, &requests, limits);
        });
        if let Err(e) = spawned {
            eprintln!("madcawl-netd: client thread: {e}");
        }
    }
}

type Writer = Arc<Mutex<UnixStream>>;

/// Sends one frame; a failed or timed-out write leaves a partial frame on
/// the stream, so the connection is shut down.
fn send(w: &Writer, f: &Frame) -> io::Result<()> {
    let mut s = w.lock().unwrap_or_else(PoisonError::into_inner);
    let r = f.write_to(&mut *s);
    if r.is_err() {
        let _ = s.shutdown(Shutdown::Both);
    }
    r
}

fn error(id: u64, message: &str) -> Frame {
    Frame::new(kind::ERROR).num(tag::ID, id).str(tag::MESSAGE, message)
}

fn serve_client(stream: UnixStream, client: &Arc<Client>, requests: &Arc<AtomicUsize>, limits: ServerLimits) -> io::Result<()> {
    let mut reader = stream.try_clone()?;
    let writer: Writer = Arc::new(Mutex::new(stream));
    let busy = Arc::new(AtomicUsize::new(0));
    while let Some(frame) = Frame::read_from(&mut reader)? {
        let id = frame.get_num(tag::ID).unwrap_or(0);
        match frame.kind {
            kind::FETCH => {
                let slots = Slot::take(&busy, limits.in_flight)
                    .and_then(|mine| Slot::take(requests, limits.requests).map(|all| (mine, all)));
                let Some(slots) = slots else {
                    send(&writer, &error(id, "too many requests in flight"))?;
                    continue;
                };
                let (client, w) = (client.clone(), writer.clone());
                let spawned = std::thread::Builder::new().name("netd-fetch".into()).spawn(move || {
                    let _slots = slots;
                    let _ = run_fetch(&client, &w, id, &frame);
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

fn parse_url(frame: &Frame, t: u8) -> Option<Url> {
    Url::parse(frame.get_str(t)?).ok()
}

fn request_of(frame: &Frame) -> Result<Request, String> {
    let url = parse_url(frame, tag::URL).ok_or("missing or bad url")?;
    let mut req = Request::get(url);
    if let Some(m) = frame.get_str(tag::METHOD) {
        if is_forbidden_method(m) {
            return Err(format!("forbidden method {m}"));
        }
        req.method = m.to_owned();
    }
    req.headers = frame.headers();
    req.body = frame.field(tag::BODY).map(<[u8]>::to_vec);
    req.navigation = frame.get_num(tag::NAVIGATION).unwrap_or(0) != 0;
    req.initiator = parse_url(frame, tag::INITIATOR);
    req.credentials = frame.get_num(tag::CREDENTIALS).unwrap_or(1) != 0;
    req.redirect = match frame.get_num(tag::REDIRECT).unwrap_or(0) {
        1 => Redirect::Manual,
        2 => Redirect::Error,
        _ => Redirect::Follow,
    };
    Ok(req)
}

/// The response as frames: HEAD, then BODY frames of up to `BODY_CHUNK`
/// bytes as the body is decoded, then END.
struct Frames<'a> {
    w: &'a Writer,
    id: u64,
    pending: Vec<u8>,
}

fn io_error(e: io::Error) -> NetError {
    NetError::Io(format!("to the content process: {e}"))
}

impl Frames<'_> {
    fn flush(&mut self) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let f = Frame::new(kind::BODY).num(tag::ID, self.id).bytes(tag::BODY, &self.pending);
        self.pending.clear();
        send(self.w, &f)
    }
}

impl Sink for Frames<'_> {
    fn head(&mut self, r: &Response) -> Result<(), NetError> {
        let mut head = Frame::new(kind::HEAD)
            .num(tag::ID, self.id)
            .num(tag::STATUS, u64::from(r.status))
            .str(tag::REASON, &r.reason);
        for u in &r.url_list {
            head = head.str(tag::URL, u.as_str());
        }
        for (n, v) in r.headers.iter().filter(|(n, _)| !is_hidden_response_header(n)) {
            head = head.header(n, v);
        }
        send(self.w, &head).map_err(io_error)
    }

    fn body(&mut self, mut chunk: &[u8]) -> Result<(), NetError> {
        while !chunk.is_empty() {
            let n = (BODY_CHUNK - self.pending.len()).min(chunk.len());
            self.pending.extend_from_slice(&chunk[..n]);
            chunk = &chunk[n..];
            if self.pending.len() == BODY_CHUNK {
                self.flush().map_err(io_error)?;
            }
        }
        Ok(())
    }
}

fn run_fetch(client: &Client, w: &Writer, id: u64, frame: &Frame) -> io::Result<()> {
    let req = match request_of(frame) {
        Ok(r) => r,
        Err(e) => return send(w, &error(id, &e)),
    };
    let mut out = Frames { w, id, pending: Vec::new() };
    if let Err(e) = client.fetch_into(&req, &mut out) {
        if let NetError::Untrusted(reason) = &e {
            eprintln!("madcawl-netd: {}: certificate not trusted: {reason}", req.url);
        }
        return send(w, &error(id, &e.to_string()));
    }
    out.flush()?;
    send(w, &Frame::new(kind::END).num(tag::ID, id))
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
