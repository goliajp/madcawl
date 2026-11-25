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
//!
//! Served with a control channel (`serve_guarded`), every connection
//! speaks for one document its host registered, and what it claims is
//! checked against that document (`guard`).
//!
//! Given a profile's downloads directory, a navigation's response that is
//! an attachment or of a type not shown as a page, and the response to a
//! fetch that asks for it, is saved there instead of going out as BODY
//! frames (`download`).

mod admit;
mod download;
mod fetch;
mod guard;
mod hello;
pub mod profile;

pub use admit::ServerLimits;
pub use download::{Downloads, is_attachment, renders, sanitize, suggested};
pub use guard::{Guard, TOKEN_LEN};
pub use profile::{Loaded, Profile, ProfileError};

use admit::{Shared, Slot, admit};
use fetch::run_fetch;
use guard::{Binding, Screened};
use hello::read_next;
use madcawl_net::cookie::{Api, SiteContext, serialize};
use madcawl_net::fetch::now_secs;
use madcawl_net::sock::Cancel;
use madcawl_net::wire::{Frame, kind, tag};
use madcawl_net::{Client, Url};
use std::io;
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

const EMFILE: i32 = 24;
const ENFILE: i32 = 23;
/// out of descriptors the pending connection stays queued, so accepting
/// again at once would spin
const ACCEPT_BACKOFF: Duration = Duration::from_millis(50);

/// Response headers a content process never sees.
pub fn is_hidden_response_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("set-cookie") || name.eq_ignore_ascii_case("set-cookie2")
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
    serve_with(streams, client, limits, None);
}

/// `serve_streams`, saving downloads into `downloads`; without it every
/// response goes to the client as BODY frames.
pub fn serve_with(
    streams: impl IntoIterator<Item = io::Result<UnixStream>>,
    client: Arc<Client>,
    limits: ServerLimits,
    downloads: Option<Arc<Downloads>>,
) {
    serve_inner(streams, Service { client, limits, downloads, guard: None });
}

/// Serves `listener` with the host's end of the control channel on
/// `control`: a connection is served only for the document its HELLO
/// token was registered for. Returns only when the control channel is
/// not a connected socket or its thread cannot start, since without it
/// no client could ever be served.
pub fn serve_guarded(
    listener: UnixListener,
    control: UnixStream,
    client: Arc<Client>,
    limits: ServerLimits,
    downloads: Option<Arc<Downloads>>,
) -> io::Result<()> {
    control.peer_addr()?;
    let guard = Arc::new(Guard::new(limits.clients));
    let g = guard.clone();
    std::thread::Builder::new().name("netd-control".into()).spawn(move || {
        if let Err(e) = g.serve_control(control) {
            eprintln!("madcawl-netd: control channel: {e}");
        }
    })?;
    serve_inner(listener.incoming(), Service { client, limits, downloads, guard: Some(guard) });
    Ok(())
}

/// What every connection is served with.
#[derive(Clone)]
struct Service {
    client: Arc<Client>,
    limits: ServerLimits,
    downloads: Option<Arc<Downloads>>,
    guard: Option<Arc<Guard>>,
}

fn serve_inner(streams: impl IntoIterator<Item = io::Result<UnixStream>>, service: Service) {
    let limits = service.limits;
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
        let (service, shared) = (service.clone(), shared.clone());
        let spawned = std::thread::Builder::new().name("netd-client".into()).spawn(move || {
            let _ = serve_client(stream, slot, &service, &shared);
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

fn serve_client(stream: UnixStream, slot: Slot, service: &Service, shared: &Shared) -> io::Result<()> {
    let (client, limits, guard) = (&service.client, service.limits, service.guard.as_deref());
    let mut reader = stream.try_clone()?;
    let writer: Writer = Arc::new(Mutex::new(stream));
    let mine = Arc::new(AtomicUsize::new(0));
    // a client that stops sending, by closing or by a bad frame, is gone
    let cancel = CancelOnDrop(Cancel::new());
    let mut binding = Binding::default();
    let deadline = Instant::now() + limits.hello_timeout;
    // declared last so it is given back before the connection closes
    let _slot = slot;
    while let Some(frame) = read_next(&mut reader, guard.is_some() && !binding.is_bound(), deadline)? {
        let id = frame.get_num(tag::ID).unwrap_or(0);
        let frame = match guard {
            Some(g) => match g.screen(&mut binding, frame) {
                Screened::Pass(f) => f,
                Screened::Reply(r) => {
                    send(&writer, &r)?;
                    if binding.is_bound() {
                        reader.set_read_timeout(None)?;
                    }
                    continue;
                }
                Screened::Close(r) => return send(&writer, &r),
            },
            None => frame,
        };
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
                let (client, w, c, d) = (client.clone(), writer.clone(), cancel.0.clone(), service.downloads.clone());
                let spawned = std::thread::Builder::new().name("netd-fetch".into()).spawn(move || {
                    let _held = held;
                    let _ = run_fetch(&client, &w, id, frame, c, d.as_deref());
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
