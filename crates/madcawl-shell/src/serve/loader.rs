//! A page's loads over the network, as a state the page's waits drive:
//! the document's response, then its stylesheets, then the commit. Every
//! request a page has in flight is here, which is what "no request in
//! flight for this page" is decided by; reading stops at a deadline with
//! what has arrived kept, so a wait that runs out can be taken up again.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use madcawl_css::style::{SheetSource, sheet_sources};
use madcawl_net::Url;
use madcawl_net::wire::{Frame, kind, tag};

use super::cookies::{Cookies, NetCookies};
use super::doc::{self, Parsed};
use super::net::{Net, NetConn};
use super::reqlog::{Entry, RequestLog};
use super::scripts::initiator;
use crate::fetch::charset_of;
use crate::load::MAX_FILE;

/// The most stylesheets one document has fetched.
pub const MAX_SHEETS: usize = 32;
/// The shortest read between two turns of a page's scripts.
const MIN_READ: Duration = Duration::from_millis(1);

pub(crate) struct Pending {
    pub what: &'static str,
    pub method: String,
    /// As logs and replies show it
    pub url: String,
    /// Query fields whose values are masked wherever the URL the load
    /// ends at is written: a password form's
    pub mask: Vec<String>,
    started: Instant,
    head_at: Option<Instant>,
    pub(super) status: Option<u16>,
    pub(super) reason: String,
    pub(super) final_url: Option<String>,
    /// The response headers, kept for a request script made
    pub(super) headers: Vec<(String, Vec<u8>)>,
    content_type: String,
    pub(super) body: Vec<u8>,
    download: Option<(String, u64)>,
    pub(super) failed: Option<String>,
    done: bool,
}

impl Pending {
    pub fn new(what: &'static str, method: &str, url: String) -> Pending {
        Pending {
            what,
            method: method.to_owned(),
            url,
            mask: Vec::new(),
            started: Instant::now(),
            head_at: None,
            status: None,
            reason: String::new(),
            final_url: None,
            headers: Vec::new(),
            content_type: String::new(),
            body: Vec::new(),
            download: None,
            failed: None,
            done: false,
        }
    }

    fn take(&mut self, f: &Frame) {
        match f.kind {
            kind::HEAD => {
                self.head_at = Some(Instant::now());
                self.status = f.get_num(tag::STATUS).map(|s| s as u16);
                self.final_url = f.all(tag::URL).last().map(|u| String::from_utf8_lossy(u).into_owned());
                let ct = f.headers().into_iter().find(|(n, _)| n.eq_ignore_ascii_case("content-type"));
                self.content_type = ct.map(|(_, v)| String::from_utf8_lossy(&v).into_owned()).unwrap_or_default();
                self.keep_head(f);
            }
            kind::BODY => {
                let b = f.field(tag::BODY).unwrap_or_default();
                if self.body.len() + b.len() > MAX_FILE as usize {
                    self.failed = Some(format!("the body is more than the {MAX_FILE} bytes a page may be"));
                    self.done = true;
                } else {
                    self.body.extend_from_slice(b);
                }
            }
            kind::DOWNLOAD => self.download = Some((f.get_str(tag::FILE).unwrap_or("").to_owned(), 0)),
            kind::DOWNLOAD_DONE => {
                let name = f.get_str(tag::FILE).unwrap_or("").to_owned();
                self.download = Some((name, f.get_num(tag::BYTES).unwrap_or(0)));
            }
            kind::END => self.done = true,
            kind::ERROR => {
                self.failed = Some(f.get_str(tag::MESSAGE).unwrap_or("the request failed").to_owned());
                self.done = true;
            }
            _ => {}
        }
    }

    fn entry(&self) -> Entry {
        let ms = |t: Instant| t.duration_since(self.started).as_secs_f64() * 1e3;
        Entry {
            what: self.what,
            method: self.method.clone(),
            url: self.url.clone(),
            status: self.status,
            bytes: self.download.as_ref().map_or(self.body.len() as u64, |d| d.1),
            head_ms: self.head_at.map(ms),
            total_ms: ms(Instant::now()),
            error: self.failed.clone(),
        }
    }

    /// The URL the request ended at, after redirects.
    pub fn final_url(&self) -> &str {
        self.final_url.as_deref().unwrap_or(&self.url)
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// The body of a response that came and is not an error.
    pub fn ok_body(&self) -> Option<&[u8]> {
        (self.failed.is_none() && self.status.is_some_and(|s| s < 400)).then_some(&self.body[..])
    }

    /// As a reply shows a request still in flight.
    pub fn line(&self) -> String {
        let ms = self.started.elapsed().as_millis();
        format!("{} {} ({} for {ms} ms)", self.method, self.url, self.what)
    }
}

#[derive(Default)]
pub(crate) enum Stage {
    #[default]
    Idle,
    /// The document request, and how long its script may run at a stretch
    Document(u64, Option<Duration>),
    /// The document's scripts run (`scripts.rs`)
    Scripts(Box<super::scripts::Scripting>),
    Sheets(Box<Parsed>),
}

/// What a load came to.
pub(crate) enum Loaded {
    /// Still in flight, or nothing was
    Pending,
    Committed(Box<Parsed>, HashMap<String, Vec<u8>>),
    Downloaded { file: String, bytes: u64 },
    Failed(String),
}

#[derive(Default)]
pub(crate) struct Loader {
    pub stage: Stage,
    pub inflight: Vec<(u64, Pending)>,
    /// `href` as written, to the bytes
    sheets: HashMap<String, u64>,
    /// What the scripts stage noted for the request log (`scripts.rs`)
    pub(crate) script_notes: Vec<String>,
    /// The network and agent session a document's `document.cookie` uses
    pub(crate) cookies: Option<(Arc<Net>, u32)>,
}

impl Loader {
    pub fn is_idle(&self) -> bool {
        matches!(self.stage, Stage::Idle) && self.inflight.iter().all(|(_, p)| p.done)
    }

    /// What is still in flight, a line each.
    pub fn lines(&self) -> Vec<String> {
        self.inflight.iter().filter(|(_, p)| !p.done).map(|(_, p)| p.line()).collect()
    }

    /// A new document request replaces whatever was loading; the
    /// document's script reaches the cookies of `cookies`' network and
    /// agent session.
    pub fn begin(&mut self, id: u64, p: Pending, cookies: (Arc<Net>, u32), script_limit: Option<Duration>, log: &mut RequestLog) {
        self.cookies = Some(cookies);
        for (_, mut old) in self.inflight.drain(..).filter(|(_, p)| !p.done) {
            old.failed = Some("replaced by a new navigation".into());
            log.push(old.entry());
        }
        self.sheets.clear();
        self.inflight.push((id, p));
        self.stage = Stage::Document(id, script_limit);
    }

    /// Reads what arrives until the load moves on or `deadline` passes.
    pub fn pump(&mut self, conn: &mut NetConn, deadline: Instant, log: &mut RequestLog) -> std::io::Result<Loaded> {
        loop {
            let out = self.advance(conn)?;
            for n in self.script_notes.drain(..) {
                log.script(n);
            }
            if let Some(out) = out {
                return Ok(out);
            }
            if self.is_idle() {
                return Ok(Loaded::Pending);
            }
            let Some(f) = conn.next(self.read_until(deadline))? else {
                if Instant::now() >= deadline { return Ok(Loaded::Pending) } else { continue }
            };
            let id = f.get_num(tag::ID).unwrap_or(0);
            if let Some((_, p)) = self.inflight.iter_mut().find(|(i, p)| *i == id && !p.done) {
                p.take(&f);
                if p.done {
                    log.push(p.entry());
                    self.drop_script_responses();
                }
            }
        }
    }

    /// Until `deadline`, or until the page's scripts have work but at
    /// least `MIN_READ`: a busy page still has its fetches read.
    fn read_until(&self, deadline: Instant) -> Instant {
        let Stage::Scripts(s) = &self.stage else { return deadline };
        s.due().map_or(deadline, |d| deadline.min(Instant::now() + d.max(MIN_READ)))
    }

    fn done(&self, id: u64) -> bool {
        self.inflight.iter().find(|(i, _)| *i == id).is_none_or(|(_, p)| p.done)
    }

    /// The next stage once what the current one waits on is in.
    pub(crate) fn advance(&mut self, conn: &mut NetConn) -> std::io::Result<Option<Loaded>> {
        match &self.stage {
            Stage::Idle => Ok(None),
            Stage::Document(id, limit) if self.done(*id) => {
                let (id, limit) = (*id, *limit);
                let i = self.inflight.iter().position(|(x, _)| *x == id).expect("the document request");
                let (_, p) = self.inflight.remove(i);
                self.stage = Stage::Idle;
                if let Some(why) = p.failed {
                    return Ok(Some(Loaded::Failed(why)));
                }
                if let Some((file, bytes)) = p.download {
                    return Ok(Some(Loaded::Downloaded { file, bytes }));
                }
                let url = p.final_url.map_or(p.url, |u| super::submit::masked(&u, &p.mask));
                let (masked, status) = (super::submit::is_masked(&url), p.status.unwrap_or(0));
                let cookies = self.cookies.clone().map(|(net, session)| Box::new(NetCookies::new(net, session)) as Box<dyn Cookies>);
                let commit = (url.clone(), masked, status);
                if let Some(s) = super::scripts::open(&p.body, &p.content_type, charset_of(&p.content_type), commit, cookies, limit) {
                    self.stage = Stage::Scripts(Box::new(s));
                    return self.advance(conn);
                }
                let (dom, encoding) = doc::parse(&p.body, &p.content_type, charset_of(&p.content_type));
                let parsed = Parsed { dom, encoding, url, masked, status };
                self.request_sheets(conn, &parsed)?;
                self.stage = Stage::Sheets(Box::new(parsed));
                self.advance(conn)
            }
            Stage::Scripts(_) => self.scripts_step(conn),
            Stage::Sheets(_) if self.sheets.values().all(|id| self.done(*id)) => {
                let Stage::Sheets(parsed) = std::mem::take(&mut self.stage) else { unreachable!() };
                let mut got = HashMap::new();
                for (href, id) in self.sheets.drain() {
                    if let Some(i) = self.inflight.iter().position(|(x, _)| *x == id) {
                        let (_, p) = self.inflight.remove(i);
                        if p.failed.is_none() && p.status.is_some_and(|s| s < 400) {
                            got.insert(href, p.body);
                        }
                    }
                }
                Ok(Some(Loaded::Committed(parsed, got)))
            }
            _ => Ok(None),
        }
    }

    /// Asks for the document's linked stylesheets over the network, as
    /// requests of the document's URL without its query or fragment.
    pub(crate) fn request_sheets(&mut self, conn: &mut NetConn, parsed: &Parsed) -> std::io::Result<()> {
        let Ok(base) = Url::parse(&parsed.url) else { return Ok(()) };
        for s in sheet_sources(&parsed.dom) {
            let SheetSource::Link { href, .. } = s else { continue };
            if self.sheets.len() == MAX_SHEETS || self.sheets.contains_key(&href) {
                continue;
            }
            let Ok(u) = base.join(href.trim()) else { continue };
            if !matches!(u.scheme(), "http" | "https") {
                continue;
            }
            let f = Frame::new(kind::FETCH).str(tag::URL, u.as_str()).str(tag::INITIATOR, &initiator(base.as_str()));
            let id = conn.fetch(f)?;
            self.inflight.push((id, Pending::new("stylesheet", "GET", u.as_str().to_owned())));
            self.sheets.insert(href, id);
        }
        Ok(())
    }
}
