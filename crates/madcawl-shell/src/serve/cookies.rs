//! `document.cookie` of a document whose scripts run: a connection of
//! its own to the network service, in the session's cookie partition as
//! the page's connection is, made the first time a turn of script reads
//! or writes a cookie and released when the turn ends, so that a page
//! holds a second registration only while its script runs. Script waits
//! for each answer, so it does not share the page's connection, where the
//! answer would queue behind response frames the loader reads. The
//! service never shows script an `HttpOnly` cookie; values are never
//! logged.
//!
//! Script waits at most `WAIT` for one call; the part of each call past
//! `PROMPT` counts against `BUDGET`, the document's slow-service time in
//! all, so a page that reads its cookies every turn never runs out while
//! the service answers promptly. Once a call fails or the budget is
//! spent, the document's cookies read as empty and writes are dropped,
//! with no new connection tried, and one line says so.

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use madcawl_net::wire::{Frame, kind, tag};

use super::net::{Net, NetConn};

/// How long script waits for one answer of the network service.
const WAIT: Duration = Duration::from_secs(1);
/// How long a document's script waits for cookies in all.
const BUDGET: Duration = Duration::from_secs(3);
/// What a call may take without counting against `BUDGET`.
const PROMPT: Duration = Duration::from_millis(50);

/// The cookie jar of a document's script.
pub trait Cookies {
    /// The cookies script may see for `url`, as `document.cookie` shows them.
    fn get(&mut self, url: &str) -> String;
    /// Stores one set-cookie-string `line` for `url` as script.
    fn set(&mut self, url: &str, line: &str);
    /// A turn of the document's script has ended.
    fn idle(&mut self);
}

/// The network service's jar, for a page of agent session `session`.
pub(crate) struct NetCookies {
    net: Arc<Net>,
    session: u32,
    /// The connection, and the network process it was made in
    conn: Option<(NetConn, u64)>,
    /// What is left of `BUDGET`
    left: Duration,
    failed: bool,
}

impl NetCookies {
    pub(crate) fn new(net: Arc<Net>, session: u32) -> NetCookies {
        NetCookies { net, session, conn: None, left: BUDGET, failed: false }
    }

    /// The service's answer to `f`; `None` once the document's cookies
    /// have failed, or when the service refused.
    fn ask(&mut self, f: Frame) -> Option<Frame> {
        if self.failed {
            return None;
        }
        let t0 = Instant::now();
        let by = t0 + WAIT.min(self.left);
        let r = self.try_ask(f, by);
        self.charge(t0);
        match r {
            Ok(r) => {
                if self.left.is_zero() {
                    self.fail(&io::Error::new(io::ErrorKind::TimedOut, "its wait for cookies is used up"), by);
                }
                (r.kind == kind::COOKIES).then_some(r)
            }
            Err(e) => {
                self.fail(&e, by);
                None
            }
        }
    }

    /// `f` on the connection held, or on a new one; a connection whose
    /// network process went away gets one more try on a new one, as a
    /// page's fetch does.
    fn try_ask(&mut self, f: Frame, by: Instant) -> io::Result<Frame> {
        if let Some((c, _)) = self.conn.as_mut() {
            match request(c, f.clone(), by) {
                Err(e) if e.kind() != io::ErrorKind::TimedOut => self.drop_conn(by),
                r => return r,
            }
        }
        let c = self.net.connect_by(self.session, Some(by))?;
        let (c, _) = self.conn.insert((c, self.net.generation()));
        request(c, f, by)
    }

    /// Counts the slow part of a call that began at `t0`.
    fn charge(&mut self, t0: Instant) {
        self.left = self.left.saturating_sub(t0.elapsed().saturating_sub(PROMPT));
    }

    fn fail(&mut self, e: &io::Error, by: Instant) {
        self.failed = true;
        self.drop_conn(by);
        eprintln!("madcawl-shell: document.cookie is off for this document: the network service: {e}");
    }

    /// Releases the connection, unless its network process has gone and
    /// taken the registration with it.
    fn drop_conn(&mut self, by: Instant) {
        if let Some((c, generation)) = self.conn.take()
            && generation == self.net.generation()
        {
            self.net.release_by(&c, Some(by));
        }
    }
}

/// Sends `f` and waits until `by` for the frame that answers it.
fn request(conn: &mut NetConn, f: Frame, by: Instant) -> io::Result<Frame> {
    let id = conn.fetch(f)?;
    loop {
        match conn.next(by)? {
            Some(r) if r.get_num(tag::ID) == Some(id) => return Ok(r),
            Some(_) => {}
            None => return Err(io::Error::new(io::ErrorKind::TimedOut, "no answer in time")),
        }
    }
}

impl Cookies for NetCookies {
    fn get(&mut self, url: &str) -> String {
        let r = self.ask(Frame::new(kind::COOKIE_GET).str(tag::URL, url));
        r.and_then(|r| r.get_str(tag::TEXT).map(str::to_owned)).unwrap_or_default()
    }

    fn set(&mut self, url: &str, line: &str) {
        let _stored = self.ask(Frame::new(kind::COOKIE_SET).str(tag::URL, url).str(tag::TEXT, line));
    }

    fn idle(&mut self) {
        let t0 = Instant::now();
        self.drop_conn(t0 + WAIT.min(self.left));
        self.charge(t0);
    }
}

impl Drop for NetCookies {
    fn drop(&mut self) {
        self.idle();
    }
}

#[cfg(test)]
#[path = "cookies_tests.rs"]
mod tests;
