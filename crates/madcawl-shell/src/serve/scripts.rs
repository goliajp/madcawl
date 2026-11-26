//! A document's scripts, run while it loads by an engine this host does
//! not name: the executable that picks one registers it
//! ([`set_scripts`]), and without one documents load as they always did,
//! with no script run. Script sources are fetched as the page's other
//! subresources are, on the page's connection to the network service:
//! in the agent session's cookie partition, in the page's request log
//! and in what a wait counts as in flight, each read under the request
//! loop's watchdog. Once the window's `load` has fired the DOM is the
//! page's document, and no script of it runs again.

use std::collections::HashMap;
use std::sync::OnceLock;

use madcawl_dom::Dom;
use madcawl_html::encoding::prescan;
use madcawl_html::{Decoder, Encoding};
use madcawl_net::Url;
use madcawl_net::wire::{Frame, kind, tag};

use super::loader::{Loaded, Loader, Parsed, Pending, Stage, initiator};
use super::net::NetConn;

/// The most script sources one document fetches; past it a script's
/// fetch fails at once, as an `error` event.
pub const MAX_SCRIPTS: usize = 256;
/// The most tasks one turn of a page's event loop runs.
const TASKS_PER_TURN: usize = 10_000;

/// A document whose scripts run as it loads.
pub trait ScriptPage {
    /// The script sources asked for since the last call: an id, and the
    /// `src` value as written.
    fn requests(&mut self) -> Vec<(u64, String)>;
    /// The source of request `id`, or `None` when the fetch failed.
    fn fetched(&mut self, id: u64, text: Option<&str>);
    /// Run the tasks that are ready, at most `max`; whether the window's
    /// `load` has fired.
    fn run(&mut self, max: usize) -> bool;
    /// The document as script left it; the engine is done with it.
    fn into_dom(self: Box<Self>) -> Dom;
}

/// Starts a document of WTF-8 `html` at `url` with its scripts run.
pub type Open = fn(html: &[u8], url: &str) -> Box<dyn ScriptPage>;

static OPEN: OnceLock<Open> = OnceLock::new();

/// Run documents' scripts with `open`'s engine, from now on in this
/// process. Set once; later calls are ignored.
pub fn set_scripts(open: Open) {
    let _first = OPEN.set(open);
}

/// The document's bytes as WTF-8 and the encoding they were in: a UTF-8
/// byte order mark, then the transport's charset, then the prescan's
/// `meta`, then windows-1252, as the encoding sniffing algorithm has it
/// (no parse started over by a late `meta`).
fn decode(body: &[u8], transport: Option<Encoding>) -> (Vec<u8>, Encoding) {
    let (body, bom) = match body.strip_prefix(b"\xef\xbb\xbf") {
        Some(rest) => (rest, Some(Encoding::Utf8)),
        None => (body, None),
    };
    let enc = bom.or(transport).or_else(|| prescan(body)).unwrap_or(Encoding::Windows1252);
    let mut out = Vec::with_capacity(body.len());
    Decoder::new(enc).decode(body, &mut out, true);
    (out, enc)
}

/// A document of `body` (`content_type` HTML) whose scripts run, when an
/// engine is registered; `commit` is its URL, whether that is masked,
/// and its status.
pub(crate) fn open(body: &[u8], content_type: &str, transport: Option<Encoding>, commit: (String, bool, u16)) -> Option<Scripting> {
    let open = OPEN.get()?;
    let essence = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if essence == "text/plain" {
        return None;
    }
    let (html, encoding) = decode(body, transport);
    let (url, masked, status) = commit;
    Some(Scripting { page: open(&html, &url), encoding, fetches: HashMap::new(), asked: 0, url, masked, status })
}

/// A document waiting for its scripts, and what its commit will say.
pub(crate) struct Scripting {
    page: Box<dyn ScriptPage>,
    encoding: Encoding,
    /// Network request id to script request id.
    fetches: HashMap<u64, u64>,
    asked: usize,
    url: String,
    masked: bool,
    status: u16,
}

impl Loader {
    /// The scripts stage: once the window has loaded, the DOM script
    /// left is parsed and waits for its stylesheets.
    pub(crate) fn scripts_step(&mut self, conn: &mut NetConn) -> std::io::Result<Option<Loaded>> {
        let Stage::Scripts(mut s) = std::mem::take(&mut self.stage) else { unreachable!("the scripts stage") };
        let base = s.url.clone();
        if !s.advance(conn, &base, &mut self.inflight)? {
            self.stage = Stage::Scripts(s);
            return Ok(None);
        }
        let s = *s;
        let (url, masked, status, encoding) = (s.url.clone(), s.masked, s.status, s.encoding);
        let parsed = Parsed { dom: s.into_dom(), encoding, url, masked, status };
        self.request_sheets(conn, &parsed)?;
        self.stage = Stage::Sheets(Box::new(parsed));
        self.advance(conn)
    }
}

impl Scripting {
    /// Asks for the sources the page wants, hands it those that have
    /// arrived, and runs it; whether its `load` has fired. `inflight` is
    /// the loader's, which the finished fetches leave.
    pub fn advance(&mut self, conn: &mut NetConn, base: &str, inflight: &mut Vec<(u64, Pending)>) -> std::io::Result<bool> {
        loop {
            let mut moved = false;
            for (id, src) in self.page.requests() {
                moved = true;
                match self.url_of(base, &src) {
                    Some(u) if self.asked < MAX_SCRIPTS => {
                        let f = Frame::new(kind::FETCH).str(tag::URL, u.as_str()).str(tag::INITIATOR, &initiator(base));
                        let net = conn.fetch(f)?;
                        inflight.push((net, Pending::new("script", "GET", u.as_str().to_owned())));
                        self.fetches.insert(net, id);
                        self.asked += 1;
                    }
                    _ => self.page.fetched(id, None),
                }
            }
            let done: Vec<u64> = self.fetches.keys().copied().filter(|n| inflight.iter().any(|(i, p)| i == n && p.is_done())).collect();
            for net in done {
                moved = true;
                let id = self.fetches.remove(&net).expect("a script fetch");
                let i = inflight.iter().position(|(x, _)| *x == net).expect("its request");
                let (_, p) = inflight.remove(i);
                let text = p.ok_body().map(String::from_utf8_lossy);
                self.page.fetched(id, text.as_deref());
            }
            if self.page.run(TASKS_PER_TURN) {
                return Ok(true);
            }
            if !moved {
                return Ok(false);
            }
        }
    }

    fn url_of(&self, base: &str, src: &str) -> Option<Url> {
        let u = Url::parse(base).ok()?.join(src.trim()).ok()?;
        matches!(u.scheme(), "http" | "https").then_some(u)
    }

    pub fn into_dom(self) -> Dom {
        let mut dom = self.page.into_dom();
        dom.trim();
        dom
    }
}
