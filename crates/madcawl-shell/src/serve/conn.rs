//! One agent session: the frames of one connection answered in order,
//! the pages it opened, and the audit line for each request.

use std::io::Write as _;
use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use madcawl_agent::{Envelope, Frame, kind, tag};

use super::limit::Bucket;
use super::log::Line;
use super::page::Page;
use super::wire::{Got, read_frame};
use super::{Shared, code, error_frame};

mod act;
mod open;

const VIEWS: [&str; 5] = ["outline", "snapshot", "markdown", "find", "extract"];

pub(crate) struct Conn<'a> {
    id: u32,
    shared: &'a Shared,
    pages: Vec<Page>,
    bucket: Bucket,
}

/// The audit log's word for how a request ended.
fn outcome(f: &Frame) -> String {
    match f.kind {
        kind::ERROR => format!("error {}", f.get_num(tag::CODE).unwrap_or(0)),
        _ => "ok".into(),
    }
}

impl<'a> Conn<'a> {
    pub fn new(id: u32, shared: &'a Shared) -> Self {
        Conn {
            id,
            shared,
            pages: Vec::new(),
            bucket: Bucket::new(shared.config.rate, shared.config.burst),
        }
    }

    fn log(&self, origin: &str, action: &str, target: (&str, &str), reply: &Frame) {
        self.shared.log.write(&Line {
            session: self.id,
            origin,
            action,
            role: target.0,
            name: target.1,
            result: &outcome(reply),
        });
    }

    pub fn run(mut self, mut s: UnixStream) {
        let max = self.shared.config.max_request;
        let mut out = Vec::new();
        loop {
            // after a frame too long or malformed the stream cannot be
            // read on: the error goes back and the session ends
            let (reply, last) = match read_frame(&mut s, max, self.next_expiry()) {
                Got::Idle if self.pages.is_empty() => {
                    self.log("-", "idle", ("", ""), &Frame::new(kind::RESULT));
                    break;
                }
                Got::Idle => {
                    self.expire();
                    continue;
                }
                Got::Closed => break,
                Got::Frame(f) => (self.admit(&f), false),
                Got::TooLong(n) => {
                    let msg = format!("a request of {n} bytes is more than the {max} one may be");
                    (error_frame(code::REQUEST_TOO_LARGE, &msg), true)
                }
                Got::Bad => (error_frame(code::BAD_REQUEST, "a malformed frame"), true),
            };
            if last {
                self.log("-", "frame", ("", ""), &reply);
            }
            out.clear();
            reply.encode(&mut out);
            if s.write_all(&out).is_err() || last {
                break;
            }
        }
        self.log_refused(true);
        for p in std::mem::take(&mut self.pages) {
            self.log(&p.url, "close-session", ("", ""), &Frame::new(kind::RESULT));
            self.drop_page(p);
        }
    }

    /// The answer, unless the session is over its request rate.
    fn admit(&mut self, f: &Frame) -> Frame {
        if self.bucket.take(Instant::now()) {
            return self.answer(f);
        }
        self.log_refused(false);
        let rate = self.shared.config.rate;
        error_frame(
            code::RATE_LIMITED,
            &format!("too many requests: at most {rate} a second; wait a moment"),
        )
    }

    /// One audit line for the requests refused over the rate, at most
    /// one a second.
    fn log_refused(&mut self, all: bool) {
        if let Some(n) = self.bucket.due(Instant::now(), all) {
            let result = format!("error {} x{n}", code::RATE_LIMITED);
            self.shared.log.write(&Line {
                session: self.id,
                origin: "-",
                action: "throttled",
                role: "",
                name: "",
                result: &result,
            });
        }
    }

    fn drop_page(&self, p: Page) {
        self.shared.owners.lock().unwrap_or_else(|e| e.into_inner()).remove(&p.id);
        p.close();
        self.shared.pages.fetch_sub(1, Ordering::SeqCst);
    }

    /// How long until the first page's idle time runs out; with no page,
    /// the idle time itself, after which the session ends.
    fn next_expiry(&self) -> Option<Duration> {
        let idle = self.shared.config.idle;
        let Some(first) = self.pages.iter().map(|p| p.last_used + idle).min() else {
            return Some(idle);
        };
        let left = first.saturating_duration_since(Instant::now());
        Some(left.max(Duration::from_millis(1)))
    }

    fn expire(&mut self) {
        let idle = self.shared.config.idle;
        let now = Instant::now();
        let (gone, kept) = std::mem::take(&mut self.pages)
            .into_iter()
            .partition(|p| p.last_used + idle <= now);
        self.pages = kept;
        for p in gone {
            self.log(&p.url, "expire", ("", ""), &Frame::new(kind::RESULT));
            self.drop_page(p);
        }
    }

    fn answer(&mut self, f: &Frame) -> Frame {
        match f.kind {
            kind::OPEN => self.open(f),
            kind::CLOSE => self.close(f),
            kind::PAGES => self.list(),
            kind::READ | kind::DIFF => self.page_request(f),
            kind::ACT | kind::WAIT => self.act(f),
            kind::REQUESTS => self.requests(f),
            _ => {
                let r = error_frame(code::UNKNOWN, "unknown request kind");
                self.log("-", &format!("kind-{}", f.kind), ("", ""), &r);
                r
            }
        }
    }

    fn page_index(&self, f: &Frame) -> Option<usize> {
        let id = f.get_num(tag::PAGE)?;
        self.pages.iter().position(|p| p.id as u64 == id)
    }

    fn no_page(&self, action: &str) -> Frame {
        let r = error_frame(
            code::NO_PAGE,
            "no such page in this session (closed, or idle too long)",
        );
        self.log("-", action, ("", ""), &r);
        r
    }

    /// `reply`'s text in a fresh envelope from `origin`, unless it is too
    /// long to send.
    fn wrap(&self, mut reply: Frame, origin: &str) -> Frame {
        let Some(at) = reply.fields.iter().position(|(t, _)| *t == tag::TEXT) else {
            return reply;
        };
        if reply.kind == kind::ERROR {
            return reply;
        }
        let max = self.shared.config.max_reply;
        let text = String::from_utf8_lossy(&reply.fields[at].1).into_owned();
        if text.len() > max {
            let msg = format!(
                "the reply would be {} bytes, more than the {max} one may be; pass a budget or a narrower scope",
                text.len()
            );
            return error_frame(code::REPLY_TOO_LARGE, &msg);
        }
        match Envelope::random() {
            Ok(e) => reply.fields[at].1 = e.wrap(origin, &text).into_bytes(),
            Err(e) => return error_frame(code::IO, &format!("no random source: {e}")),
        }
        reply
    }

    fn page_request(&mut self, f: &Frame) -> Frame {
        let action = match f.kind {
            kind::DIFF => "diff",
            _ => VIEWS
                .get(f.get_num(tag::VIEW).unwrap_or(1) as usize)
                .copied()
                .unwrap_or("snapshot"),
        };
        let i = match self.page_for(f, action) {
            Ok(i) => i,
            Err(r) => return r,
        };
        let p = &mut self.pages[i];
        let subtree = f.kind == kind::READ && f.get_num(tag::SCOPE) == Some(2);
        let target = f
            .get_str(tag::REF)
            .filter(|_| subtree)
            .and_then(|r| p.target(r));
        let reply = p
            .handle(f)
            .unwrap_or_else(|| error_frame(code::UNKNOWN, "unknown request kind"));
        let url = p.url.clone();
        let reply = self.wrap(reply, &url);
        let (role, name) = target.unwrap_or(("", String::new()));
        self.log(&url, action, (role, &name), &reply);
        reply
    }

    fn close(&mut self, f: &Frame) -> Frame {
        let i = match self.page_for(f, "close") {
            Ok(i) => i,
            Err(r) => return r,
        };
        let p = self.pages.remove(i);
        let reply = Frame::new(kind::RESULT).str(tag::TEXT, &format!("closed p{}", p.id));
        self.log(&p.url, "close", ("", ""), &reply);
        self.drop_page(p);
        reply
    }

    fn list(&self) -> Frame {
        let now = Instant::now();
        let mut text = String::new();
        for p in &self.pages {
            text.push_str(&format!(
                "p{} {} {}x{} idle {}s\n",
                p.id,
                p.url,
                p.width,
                p.height,
                now.duration_since(p.last_used).as_secs()
            ));
        }
        if text.is_empty() {
            text.push_str("no pages open\n");
        }
        let reply = Frame::new(kind::RESULT).str(tag::TEXT, &text);
        self.log("-", "pages", ("", ""), &reply);
        reply
    }
}
