//! One windowless page the host holds for an agent session: its current
//! document, styled for the viewport the agent asked for and laid out
//! once asked where things are, the one agent session on it the host
//! speaks for, its history, its loads in flight and its request log.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::time::{Duration, Instant};

use madcawl_agent::{Agents, Frame, SessionId, role_and_name, tag};
use madcawl_dom::NodeId;
use madcawl_html::Encoding;
use madcawl_net::Url;
use madcawl_net::submit::{Navigation, fetch_frame};

use super::doc::{Doc, HeldSheets};
use super::loader::{Loaded, Loader, Pending};
use super::net::{Net, NetConn};
use super::reqlog::{Entry, RequestLog};
use super::target;
use crate::load;
use crate::sheet_files::FileSheets;

/// What a page may do, from the host's configuration.
pub(crate) struct Env<'a> {
    pub allow: &'a [PathBuf],
    pub net: Option<&'a Arc<Net>>,
    /// Files an upload may pick, by directory
    pub uploads: &'a [PathBuf],
    /// Origins a form with a password field may be submitted to
    pub approve: &'a [String],
    /// The agent session (connection) the page is of
    pub owner: u32,
    /// What tells this run of the host from others, for the names of
    /// what it leaves behind: session numbers start again every run
    pub run: &'a str,
}

/// A navigation to start.
pub(crate) struct Request {
    pub method: &'static str,
    pub url: Url,
    pub content_type: Option<String>,
    pub body: Option<Vec<u8>>,
    /// The URL as logs show it, with secrets masked
    pub shown: String,
    /// The history entry it goes to, for back and forward
    pub entry: Option<usize>,
    /// Query fields masked in the URL the load ends at
    pub mask: Vec<String>,
    /// When not empty, the only origins a redirect may carry the body to
    pub body_origins: Vec<String>,
}

/// A history entry. One a GET form with a password led to keeps only
/// its masked URL, so it is never loaded again: that would send the mask
/// as the password.
pub(crate) struct Visit {
    pub url: String,
    pub masked: bool,
}

pub struct Page {
    pub id: u32,
    pub url: String,
    pub width: f32,
    pub height: f32,
    owner: u32,
    pub last_used: Instant,
    pub(crate) doc: Doc,
    pub(crate) agents: Agents,
    pub(crate) session: SessionId,
    pub(crate) conn: Option<NetConn>,
    net: Option<Arc<Net>>,
    pub(crate) loader: Loader,
    pub(crate) log: RequestLog,
    pub(crate) history: Vec<Visit>,
    pub(crate) at: usize,
    /// The history entry the load in flight goes to
    going_to: Option<usize>,
    pub(crate) focus: Option<NodeId>,
}

impl Page {
    /// A page on a blank document; `goto` loads what it is for. The
    /// agent session starts on the first document it loads.
    pub(crate) fn blank(id: u32, size: (f32, f32), net: Option<&Arc<Net>>, owner: u32) -> Page {
        let (dom, encoding) = load::parse(b"", Some(Encoding::Utf8));
        let mut links = FileSheets::new(None, None);
        let doc = Doc::new(dom, encoding, size, (owner, "about:blank"), &mut links);
        Page {
            id,
            url: "about:blank".into(),
            width: size.0,
            height: size.1,
            owner,
            last_used: Instant::now(),
            doc,
            agents: Agents::sharing_generations(&PAGE_GENS),
            session: 0,
            conn: None,
            net: net.cloned(),
            loader: Loader::default(),
            log: RequestLog::default(),
            history: Vec::new(),
            at: 0,
            going_to: None,
            focus: None,
        }
    }

    /// The page's agent session, started on the current document if it
    /// has none yet.
    fn ensure(&mut self) {
        if self.agents.is_attached() {
            return;
        }
        let (dom, inputs) = self.doc.parts();
        self.session = self.agents.attach(dom, inputs).expect("a page has one agent session, under the limit");
    }

    /// Opens a local file now, a charset given or not.
    pub(crate) fn load_file(&mut self, path: &Path, url: String, charset: Option<Encoding>, allow: &[PathBuf]) -> std::io::Result<()> {
        let started = Instant::now();
        let bytes = load::read_file(path, load::MAX_FILE)?;
        let (dom, encoding) = load::parse(&bytes, charset);
        let base = path.parent().map(Path::to_path_buf);
        let mut links = FileSheets::new(base.as_deref(), Some(allow));
        let size = (self.width, self.height);
        let doc = Doc::new(dom, encoding, size, (self.owner, &url), &mut links);
        self.log.push(Entry {
            what: "document",
            method: "GET".into(),
            url: url.clone(),
            status: None,
            bytes: bytes.len() as u64,
            head_ms: None,
            total_ms: started.elapsed().as_secs_f64() * 1e3,
            error: None,
        });
        self.commit(doc, url, false);
        Ok(())
    }

    /// Starts a navigation: a local file loads now, a network one is put
    /// in flight for a wait to finish.
    pub(crate) fn goto(&mut self, r: Request, env: &Env) -> Result<(), String> {
        self.going_to = r.entry;
        if r.url.scheme() == "file" {
            let (path, url) = target::resolve(r.url.as_str(), env.allow).map_err(|e| e.message())?;
            return self.load_file(&path, url, None, env.allow).map_err(|e| format!("cannot open the page: {e}"));
        }
        if !matches!(r.url.scheme(), "http" | "https") {
            return Err(format!("`{}:` URLs are not navigated to", r.url.scheme()));
        }
        let Some(net) = self.net.clone() else {
            return Err("this host does not fetch pages from the network (start it with --net)".into());
        };
        if self.conn.is_none() {
            self.conn = Some(net.connect(env.owner).map_err(|e| format!("the network service: {e}"))?);
        }
        let initiator = Url::parse(&self.url).ok().filter(|u| matches!(u.scheme(), "http" | "https"));
        let nav = Navigation {
            id: 0,
            method: r.method,
            url: &r.url,
            content_type: r.content_type.as_deref(),
            body: r.body.as_deref(),
            initiator: initiator.as_ref(),
            body_origins: &r.body_origins,
        };
        let conn = self.conn.as_mut().expect("connected");
        let id = conn.fetch(fetch_frame(&nav)).map_err(|e| format!("the network service: {e}"))?;
        let mut p = Pending::new("document", r.method, r.shown);
        p.mask = r.mask;
        self.loader.begin(id, p, &mut self.log);
        Ok(())
    }

    /// Drives the loads in flight until they finish or `deadline`; what
    /// a finished navigation came to, for the reply.
    pub(crate) fn settle(&mut self, deadline: Instant, env: &Env) -> Option<String> {
        let conn = self.conn.as_mut()?;
        let loaded = match self.loader.pump(conn, deadline, &mut self.log) {
            Ok(l) => l,
            Err(e) => {
                self.loader = Loader::default();
                self.conn = None;
                return Some(format!("the load failed: the network service: {e}"));
            }
        };
        match loaded {
            Loaded::Pending => None,
            Loaded::Failed(why) => Some(format!("the load failed: {why}")),
            Loaded::Downloaded { file, bytes } => Some(self.downloaded(&file, bytes, env)),
            Loaded::Committed(p, sheets) => {
                let size = (self.width, self.height);
                let mut links = HeldSheets(&sheets);
                let doc = Doc::new(p.dom, p.encoding, size, (self.owner, &p.url), &mut links);
                let status = p.status;
                self.commit(doc, p.url, p.masked);
                Some(format!("navigated to {} (status {status})", self.url))
            }
        }
    }

    /// A download the network service saved, moved into the session's
    /// own directory.
    fn downloaded(&mut self, file: &str, bytes: u64, env: &Env) -> String {
        let Some(dir) = env.net.and_then(|n| n.downloads.clone()) else {
            return format!("downloaded {file} ({bytes} bytes)");
        };
        let mine = dir.join(format!("s{}-{}", env.owner, env.run));
        let to = mine.join(file);
        match std::fs::create_dir_all(&mine).and_then(|()| std::fs::rename(dir.join(file), &to)) {
            Ok(()) => format!("downloaded {file} ({bytes} bytes) to {}", to.display()),
            Err(e) => format!("downloaded {file} ({bytes} bytes); moving it failed: {e}"),
        }
    }

    /// Makes `doc` the page's document: references to the old one go
    /// stale, the history moves.
    fn commit(&mut self, doc: Doc, url: String, masked: bool) {
        self.doc = doc;
        if self.agents.is_attached() {
            let (dom, inputs) = self.doc.parts();
            self.agents.navigated(dom, inputs);
        } else {
            self.ensure();
        }
        match self.going_to.take() {
            Some(i) if i < self.history.len() => {
                self.at = i;
                self.history[i] = Visit { url: url.clone(), masked };
            }
            _ => {
                if !self.history.is_empty() {
                    self.history.truncate(self.at + 1);
                }
                if self.history.len() == MAX_HISTORY {
                    self.history.remove(0);
                }
                self.history.push(Visit { url: url.clone(), masked });
                self.at = self.history.len() - 1;
            }
        }
        self.url = url;
        self.focus = None;
    }

    /// Answers a view or diff request as the page's own session, whatever
    /// session the frame names.
    pub fn handle(&mut self, f: &Frame) -> Option<Frame> {
        self.last_used = Instant::now();
        self.ensure();
        let mut f = f.clone();
        f.fields.retain(|(t, _)| !matches!(*t, tag::SESSION | tag::PAGE));
        let f = f.num(tag::SESSION, self.session as u64);
        self.lay_out_for(&f);
        let f = self.floored(f);
        let (dom, inputs) = self.doc.parts();
        self.agents.handle(dom, inputs, &f)
    }

    /// Brings the semantic tree up to date; the sequence it is at.
    pub(crate) fn sync(&mut self) -> u64 {
        self.ensure();
        let (dom, inputs) = self.doc.parts();
        self.agents.sync(dom, inputs)
    }

    /// The node a reference names, if it still does.
    pub(crate) fn resolve(&mut self, r: &str) -> Result<NodeId, madcawl_agent::AgentError> {
        self.ensure();
        let (dom, inputs) = self.doc.parts();
        self.agents.resolve(self.session, dom, inputs, r)
    }

    /// The role and name of the element a reference names, for the audit
    /// log.
    pub fn target(&mut self, r: &str) -> Option<(&'static str, String)> {
        let n = self.resolve(r).ok()?;
        let (role, name) = role_and_name(self.agents.tree()?, n)?;
        Some((role.name(), name))
    }

    pub fn close(mut self) {
        self.agents.detach(self.session, &mut self.doc.dom);
        if let (Some(net), Some(conn)) = (&self.net, &self.conn) {
            net.release(conn);
        }
    }
}

/// Page generations of every page of the process, so a reference names a
/// node of one page only: given for another page, it is stale.
static PAGE_GENS: AtomicU32 = AtomicU32::new(0);

/// The most history entries a page keeps; the oldest go first.
pub const MAX_HISTORY: usize = 64;

/// How long a page given no timeout waits for its first load.
pub const OPEN_WAIT: Duration = Duration::from_secs(30);
