//! The requests and responses the agent protocol carries, as frames, and
//! answering a request in process (`Agents::handle`), which the proxy
//! does for each frame once it has checked the session's permissions
//! and before it puts the text in an envelope.

use madcawl_dom::Dom;

use crate::agents::{AgentError, Agents, SessionId, ViewKind};
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
    pub const RESULT: u16 = 20;
    pub const ERROR: u16 = 21;
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
}

/// The views by number on the wire.
const VIEWS: [&str; 4] = ["outline", "snapshot", "markdown", "find"];
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
}
