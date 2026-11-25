//! Which document a content process speaks for, decided by its host and
//! not by the content process.
//!
//! A content process is not trusted: a compromised one can put any URL in
//! a frame. What it claims (the initiator of a request, the document a
//! `document.cookie` call is for) is therefore checked against a record
//! the content process cannot write. The host that spawns the network
//! process keeps the other end of a socket pair, the control channel,
//! which it hands over at spawn time (`--control-fd`); nothing else can
//! reach that socket, so no path name has to be guarded. On it the host
//! registers each top-level document and gets back a random 128-bit
//! token, which it passes to that document's content process. The content
//! process opens its connection with HELLO and the token; from then on
//! every frame on the connection is checked against the registered
//! document's URL, which the host changes when it commits a navigation.
//! A token on the wire carries no authority of its own, so a guessed or
//! stale one is only refused. The token is issued by the network process
//! rather than by the host so that its randomness is not the host's to
//! get wrong.
//!
//! The check is by schemeful site (scheme and registrable domain): a
//! request's claimed initiator and a cookie call's URL must be same-site
//! with the document. A missing initiator, which would mean the user, is
//! replaced by the document. A content process never navigates: a FETCH
//! from it that claims to be a navigation is refused, and its credentials
//! field may only lower credentials (the lowest of its values holds).
//! Navigations are fetched by the host itself, on a connection whose
//! token it registered with `NAVIGATION` set instead of a URL; frames on
//! such a connection go through as sent, and the token never leaves the
//! host. Same-site frames of another origin share the document's
//! registration until frames are registered on their own.
//!
//! Each registration is in a cookie partition the host names, the
//! profile's own jar when it names none (`partition`). Dropping a
//! partition drops its cookies and its registrations with it.
//!
//! A connection must open with a HELLO that shows a registered token;
//! any other first frame, a refused HELLO, a second HELLO or any frame
//! after the host dropped the registration is answered with one refusal
//! and the connection is closed, so a client without a document cannot
//! keep a client slot. Every refusal is an ERROR frame with `REFUSED` set
//! and is counted, in all and per document; STATS on the control channel
//! reads the counts. A refusal is logged only when it is the first on its
//! connection and the first in `LOG_INTERVAL` for its document, or for
//! all connections without one, since a client may reconnect to be
//! refused again. Registrations are bounded by the client limit.

mod control;
mod fetch;
mod partition;

use crate::error;
use madcawl_net::cookie::site_of;
use madcawl_net::partition::PartitionId;
use madcawl_net::{Client, Url};
use madcawl_net::wire::{Frame, kind, tag};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

pub const TOKEN_LEN: usize = 16;
type Token = [u8; TOKEN_LEN];

const LOG_INTERVAL: Duration = Duration::from_secs(1);

struct Doc {
    /// `None` for the host's own connection
    url: Option<Url>,
    partition: PartitionId,
    refused: u64,
    /// when a refusal for it was last logged, as `Guard::now` gives it
    logged: u64,
}

pub struct Guard {
    docs: Mutex<HashMap<Token, Doc>>,
    max: usize,
    /// whose cookie partitions registrations open and drop
    client: Arc<Client>,
    refused: AtomicU64,
    born: Instant,
    /// when a refusal without a document was last logged, as `now` gives it
    unbound_logged: AtomicU64,
    lines: AtomicU64,
}

/// The token a connection showed in its HELLO, and whether a refusal on
/// it was logged.
#[derive(Default)]
pub(crate) struct Binding {
    token: Option<Token>,
    logged: bool,
}

impl Binding {
    pub(crate) fn is_bound(&self) -> bool {
        self.token.is_some()
    }
}

pub(crate) enum Screened {
    Pass(Frame),
    Reply(Frame),
    /// answer and close the connection
    Close(Frame),
}

unsafe extern "C" {
    fn getentropy(buf: *mut u8, len: usize) -> i32;
}

fn new_token() -> Token {
    let mut t = [0u8; TOKEN_LEN];
    // SAFETY: the buffer is writable for its length, which is under getentropy's 256-byte limit
    let r = unsafe { getentropy(t.as_mut_ptr(), t.len()) };
    assert_eq!(r, 0, "getentropy");
    t
}

/// The frame's token: `Ok(None)` when it has none, `Err` when it has one
/// of the wrong length.
fn token_of(f: &Frame) -> Result<Option<Token>, ()> {
    f.field(tag::TOKEN).map(|t| t.try_into().map_err(|_| ())).transpose()
}

/// The lowest of a frame's values for a flag field, or `None` when it has
/// none; `Err` for a value other than 0 and 1.
fn flag_of(f: &Frame, t: u8) -> Result<Option<u64>, ()> {
    let mut low = None;
    for raw in f.all(t) {
        let v = match raw {
            [0] => 0,
            [1] => 1,
            _ => return Err(()),
        };
        low = Some(low.map_or(v, |l: u64| l.min(v)));
    }
    Ok(low)
}

fn log_due(last: u64, now: u64) -> bool {
    last == 0 || now.saturating_sub(last) >= LOG_INTERVAL.as_millis() as u64
}

fn same_site(a: &Url, b: &Url) -> bool {
    match (a.host_str(), b.host_str()) {
        (Some(x), Some(y)) => a.scheme() == b.scheme() && site_of(x) == site_of(y),
        _ => false,
    }
}

/// A FETCH or cookie frame checked against the registered document; the
/// host's own connection, which has none, sends what it means.
fn for_document(doc: Option<&Url>, frame: Frame) -> Result<Frame, &'static str> {
    let Some(url) = doc else { return Ok(frame) };
    if frame.kind == kind::FETCH {
        return fetch::screen(url, frame);
    }
    match frame.get_str(tag::URL).and_then(|s| Url::parse(s).ok()) {
        Some(u) if same_site(&u, url) => Ok(frame),
        _ => Err("cookies of another site than the document"),
    }
}

impl Guard {
    /// A guard that holds at most `max` registered documents, in the
    /// cookie partitions of `client`.
    pub fn new(max: usize, client: Arc<Client>) -> Self {
        Guard {
            docs: Mutex::new(HashMap::new()),
            max,
            client,
            refused: AtomicU64::new(0),
            born: Instant::now(),
            unbound_logged: AtomicU64::new(0),
            lines: AtomicU64::new(0),
        }
    }

    /// Refusals so far, of all clients.
    pub fn refused(&self) -> u64 {
        self.refused.load(Ordering::SeqCst)
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<Token, Doc>> {
        self.docs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Counts a refusal and logs it when it is the first on its connection
    /// and the first in a while for its document, or for all without one.
    fn refuse(&self, binding: &mut Binding, doc: Option<&mut Doc>, id: u64, why: &str) -> Frame {
        self.refused.fetch_add(1, Ordering::SeqCst);
        let first = !std::mem::replace(&mut binding.logged, true);
        let on = match doc {
            Some(d) => {
                d.refused += 1;
                let due = first && log_due(d.logged, self.now());
                d.logged = if due { self.now() } else { d.logged };
                due.then(|| d.url.as_ref().map_or("the host", Url::as_str).to_owned())
            }
            None => (first && self.unbound_log_due()).then(|| "an unregistered client".to_owned()),
        };
        if let Some(on) = on {
            self.lines.fetch_add(1, Ordering::SeqCst);
            eprintln!("madcawl-netd: refused for {on}: {why}");
        }
        error(id, &format!("refused: {why}")).num(tag::REFUSED, 1)
    }

    /// Milliseconds since the guard was made, plus one: zero is never.
    fn now(&self) -> u64 {
        self.born.elapsed().as_millis() as u64 + 1
    }

    fn unbound_log_due(&self) -> bool {
        let (last, now) = (self.unbound_logged.load(Ordering::SeqCst), self.now());
        log_due(last, now) && self.unbound_logged.compare_exchange(last, now, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }

    /// Binds a connection by its first frame, which must be a HELLO with a
    /// registered token.
    fn hello(&self, binding: &mut Binding, frame: &Frame, id: u64) -> Screened {
        if frame.kind != kind::HELLO {
            return Screened::Close(self.refuse(binding, None, id, "a first frame other than HELLO"));
        }
        match token_of(frame) {
            Ok(Some(t)) if self.lock().contains_key(&t) => {
                binding.token = Some(t);
                Screened::Reply(Frame::new(kind::HELLO).num(tag::ID, id).num(tag::OK, 1))
            }
            _ => Screened::Close(self.refuse(binding, None, id, "a HELLO without a registered token")),
        }
    }

    /// Lets a frame from a content process through, rewritten where the
    /// document decides, or answers it with a refusal.
    pub(crate) fn screen(&self, binding: &mut Binding, frame: Frame) -> Screened {
        let id = frame.get_num(tag::ID).unwrap_or(0);
        let Some(token) = binding.token else { return self.hello(binding, &frame, id) };
        let mut docs = self.lock();
        let Some(doc) = docs.get_mut(&token) else {
            return Screened::Close(self.refuse(binding, None, id, "a registration the host has dropped"));
        };
        let refused = match frame.kind {
            kind::HELLO => return Screened::Close(self.refuse(binding, Some(doc), id, "a second HELLO")),
            kind::REGISTER | kind::UNREGISTER | kind::STATS | kind::PARTITION_DROP => "a control frame from a content process",
            kind::FETCH | kind::COOKIE_GET | kind::COOKIE_SET => {
                match partition::screen(doc.partition, frame).and_then(|f| for_document(doc.url.as_ref(), f)) {
                    Ok(f) => return Screened::Pass(f),
                    Err(why) => why,
                }
            }
            _ => return Screened::Pass(frame),
        };
        Screened::Reply(self.refuse(binding, Some(doc), id, refused))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use madcawl_net::Config;

    fn client() -> Arc<Client> {
        Arc::new(Client::new(Config::default()))
    }

    fn register(g: &Guard, url: &str) -> Vec<u8> {
        let reg = g.control(&Frame::new(kind::REGISTER).num(tag::ID, 1).str(tag::URL, url)).unwrap();
        reg.field(tag::TOKEN).unwrap().to_vec()
    }

    fn bound(g: &Guard, url: &str) -> Binding {
        hello(g, &register(g, url))
    }

    fn hello(g: &Guard, token: &[u8]) -> Binding {
        let hello = Frame::new(kind::HELLO).num(tag::ID, 1).bytes(tag::TOKEN, token);
        let mut b = Binding::default();
        assert!(matches!(g.screen(&mut b, hello), Screened::Reply(_)));
        b
    }

    #[test]
    fn refusals_are_all_counted_but_logged_once_per_connection() {
        let g = Guard::new(4, client());
        let mut b = bound(&g, "http://127.0.0.1/");
        for i in 0..5 {
            let f = Frame::new(kind::COOKIE_GET).num(tag::ID, i).str(tag::URL, "https://bank.example.com/");
            assert!(matches!(g.screen(&mut b, f), Screened::Reply(_)));
        }
        // strangers, each on its own connection, share one line per interval
        for _ in 0..5 {
            let f = Frame::new(kind::COOKIE_GET).num(tag::ID, 9);
            assert!(matches!(g.screen(&mut Binding::default(), f), Screened::Close(_)));
        }
        assert_eq!((g.refused(), g.lines.load(Ordering::SeqCst)), (10, 2));
    }

    #[test]
    fn a_document_that_reconnects_to_be_refused_is_logged_once_per_interval() {
        let g = Guard::new(4, client());
        let token = register(&g, "http://127.0.0.1/");
        for i in 0..5 {
            let mut b = hello(&g, &token);
            let f = Frame::new(kind::COOKIE_GET).num(tag::ID, i).str(tag::URL, "https://bank.example.com/");
            assert!(matches!(g.screen(&mut b, f), Screened::Reply(_)));
        }
        assert_eq!((g.refused(), g.lines.load(Ordering::SeqCst)), (5, 1));
    }
}
