//! The requests and responses the agent protocol carries, as frames, and
//! answering a request in process (`Agents::handle`), which the proxy
//! does for each frame once it has checked the session's permissions
//! and before it puts the text in an envelope.

use madcawl_dom::Dom;

use crate::agents::{AgentError, Agents, SessionId, ViewKind};
use crate::extract::{ExtractOpts, Format, What};
use crate::frame::Frame;
use crate::role::Role;
use crate::tree::Inputs;
use crate::view::{Query, Scope, ViewOpts};

pub mod kind {
    pub const ATTACH: u16 = 1;
    pub const ATTACHED: u16 = 2;
    pub const DETACH: u16 = 3;
    pub const READ: u16 = 10;
    pub const DIFF: u16 = 11;
    /// An action on the page by reference (`act::Action`), answered once
    /// the page is stable or the wait ran out
    pub const ACT: u16 = 12;
    /// Waiting for the page to be stable, with no action
    pub const WAIT: u16 = 13;
    pub const RESULT: u16 = 20;
    pub const ERROR: u16 = 21;
    /// The host's own requests, answered by the proxy, not by a page
    pub const OPEN: u16 = 30;
    pub const OPENED: u16 = 31;
    pub const CLOSE: u16 = 32;
    pub const PAGES: u16 = 33;
    /// A page's log of requests and load errors
    pub const REQUESTS: u16 = 34;
}

pub mod tag {
    pub const SESSION: u8 = 1;
    pub const VIEW: u8 = 2;
    pub const SCOPE: u8 = 3;
    pub const REF: u8 = 4;
    pub const BUDGET: u8 = 5;
    pub const HIDDEN: u8 = 6;
    pub const TEXT: u8 = 7;
    pub const ROLE: u8 = 8;
    pub const LIMIT: u8 = 9;
    pub const SINCE: u8 = 10;
    pub const SEQ: u8 = 11;
    pub const TOKENS: u8 = 12;
    pub const FOLDED: u8 = 13;
    pub const CODE: u8 = 14;
    /// The page a request is for, by the number `OPENED` gave
    pub const PAGE: u8 = 15;
    /// What to open: an absolute path or a `file:` URL
    pub const URL: u8 = 16;
    pub const WIDTH: u8 = 17;
    pub const HEIGHT: u8 = 18;
    /// The charset label a transport would have given
    pub const CHARSET: u8 = 19;
    /// What an extraction takes: `table`, `list`, `links`, `images`,
    /// `forms`
    pub const WHAT: u8 = 20;
    /// `json` (the default, a JSON object a line) or `markdown`
    pub const FORMAT: u8 = 21;
    /// Records an extraction skips
    pub const OFFSET: u8 = 22;
    /// An action's name: `navigate`, `back`, `forward`, `reload`,
    /// `scroll`, `click`, `hover`, `type`, `press`, `select`, `check`,
    /// `upload`, `fill_form`, `submit`
    pub const ACTION: u8 = 23;
    /// What an action takes: text to type, a key, a URL, an option, a
    /// path, a scroll direction
    pub const VALUE: u8 = 24;
    /// One field of a `fill_form`, repeated: the reference, a NUL, the
    /// value
    pub const FIELD: u8 = 25;
    /// How long to wait for the page to be stable, in milliseconds
    pub const TIMEOUT: u8 = 26;
    /// Wait also until this reference names a node
    pub const UNTIL_REF: u8 = 27;
    /// Wait also until this text is on the page
    pub const UNTIL_TEXT: u8 = 28;
    /// Viewports to scroll by
    pub const AMOUNT: u8 = 29;
    /// 1 to tick a box, 0 to clear it; for `fill_form`, 1 to submit
    pub const ON: u8 = 30;
    /// 1 when the page was stable when the reply was made
    pub const STABLE: u8 = 31;
    /// Microseconds from the request to the action started, of waiting
    /// for the page to be stable, of writing the diff
    pub const DISPATCH_US: u8 = 32;
    pub const WAIT_US: u8 = 33;
    pub const DIFF_US: u8 = 34;
    /// Of the dispatch and the wait, microseconds spent laying the
    /// document out: the first question of geometry on a document lays it
    /// out
    pub const LAYOUT_US: u8 = 35;
}

/// The views by number on the wire; `extract` is a view of records.
const VIEWS: [&str; 5] = ["outline", "snapshot", "markdown", "find", "extract"];
/// The scopes by number on the wire.
const SCOPES: [&str; 4] = ["page", "viewport", "subtree", "main"];

/// A request frame for a view: `view` and `scope` by name.
pub fn read_request(session: SessionId, view: &str, scope: &str, reference: &str) -> Frame {
    let v = VIEWS.iter().position(|&x| x == view).unwrap_or(1);
    let s = SCOPES.iter().position(|&x| x == scope).unwrap_or(0);
    Frame::new(kind::READ)
        .num(tag::SESSION, session as u64)
        .num(tag::VIEW, v as u64)
        .num(tag::SCOPE, s as u64)
        .str(tag::REF, reference)
}

fn error(e: AgentError) -> Frame {
    let code = match e {
        AgentError::NoSession => 1,
        AgentError::Ref(crate::refs::RefError::Malformed) => 2,
        AgentError::Ref(crate::refs::RefError::Stale) => 3,
        AgentError::TooFarBehind => 4,
        AgentError::TooManySessions => 5,
        // the host's code for a request it cannot take, so both agree
        AgentError::BadRequest(_) => 10,
    };
    Frame::new(kind::ERROR)
        .num(tag::CODE, code)
        .str(tag::TEXT, e.message())
}

fn result(seq: u64, v: crate::view::View) -> Frame {
    Frame::new(kind::RESULT)
        .num(tag::SEQ, seq)
        .num(tag::TOKENS, v.tokens as u64)
        .num(tag::FOLDED, v.folded as u64)
        .str(tag::TEXT, &v.text)
}

impl Agents {
    /// Answers one request frame; `None` for a kind this side does not
    /// know, which the caller skips.
    pub fn handle(&mut self, dom: &mut Dom, inputs: Inputs, f: &Frame) -> Option<Frame> {
        let session = f.get_num(tag::SESSION).unwrap_or(0) as SessionId;
        Some(match f.kind {
            kind::ATTACH => match self.attach(dom, inputs) {
                Ok(id) => Frame::new(kind::ATTACHED)
                    .num(tag::SESSION, id as u64)
                    .num(tag::SEQ, self.seq()),
                Err(e) => error(e),
            },
            kind::DETACH => {
                self.detach(session, dom);
                Frame::new(kind::RESULT).num(tag::SEQ, self.seq())
            }
            kind::READ => match self.read(session, dom, inputs, f) {
                Ok((seq, v)) => result(seq, v),
                Err(e) => error(e),
            },
            kind::DIFF => {
                let since = f.get_num(tag::SINCE).unwrap_or(0);
                match self.diff(session, dom, inputs, since) {
                    Ok((seq, v)) => result(seq, v),
                    Err(e) => error(e),
                }
            }
            _ => return None,
        })
    }

    fn read(
        &mut self,
        session: SessionId,
        dom: &mut Dom,
        inputs: Inputs,
        f: &Frame,
    ) -> Result<(u64, crate::view::View), AgentError> {
        let scope = match f.get_num(tag::SCOPE).unwrap_or(0) {
            1 => Scope::Viewport,
            2 => {
                let r = f.get_str(tag::REF).unwrap_or("");
                Scope::Subtree(self.resolve(session, dom, inputs, r)?)
            }
            3 => Scope::Main,
            _ => Scope::Page,
        };
        if f.get_num(tag::VIEW) == Some(4) {
            return self.extract_request(session, dom, inputs, f, scope);
        }
        let opts = ViewOpts {
            scope,
            budget: f.get_num(tag::BUDGET).map(|b| b as usize),
            include_hidden: f.get_num(tag::HIDDEN).unwrap_or(0) != 0,
        };
        let kind = match f.get_num(tag::VIEW).unwrap_or(1) {
            0 => ViewKind::Outline,
            2 => ViewKind::Markdown,
            3 => ViewKind::Find(Query {
                text: f.get_str(tag::TEXT).map(str::to_owned),
                role: f.get_str(tag::ROLE).and_then(Role::from_aria),
                limit: f.get_num(tag::LIMIT).unwrap_or(0) as usize,
            }),
            _ => ViewKind::Snapshot,
        };
        self.view(session, dom, inputs, &kind, opts)
    }

    fn extract_request(
        &mut self,
        session: SessionId,
        dom: &mut Dom,
        inputs: Inputs,
        f: &Frame,
        scope: Scope,
    ) -> Result<(u64, crate::view::View), AgentError> {
        let what = match f.get_str(tag::WHAT) {
            None => What::Table,
            Some(w) => What::from_name(w).ok_or(AgentError::BadRequest(
                "unknown what: table, list, links, images or forms",
            ))?,
        };
        let mut opts = ExtractOpts::new(what);
        // a reference names the target: the table, an item, the subtree
        match scope {
            Scope::Subtree(n) => opts.target = Some(n),
            s => opts.scope = s,
        }
        opts.offset = f.get_num(tag::OFFSET).unwrap_or(0) as usize;
        opts.budget = f.get_num(tag::BUDGET).map(|b| b as usize);
        opts.include_hidden = f.get_num(tag::HIDDEN).unwrap_or(0) != 0;
        opts.format = match f.get_str(tag::FORMAT).map(str::trim) {
            None | Some("" | "json") => Format::JsonLines,
            Some("markdown") => Format::Markdown,
            Some(_) => return Err(AgentError::BadRequest("unknown format: json or markdown")),
        };
        self.extract(session, dom, inputs, &opts)
    }
}
