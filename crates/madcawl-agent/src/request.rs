//! What an agent session asks of a page's agents, and how a request
//! fails.

use crate::refs::RefError;
use crate::view::Query;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentError {
    NoSession,
    Ref(RefError),
    /// The diff reaches back past what the session's ring holds
    TooFarBehind,
    /// The page has `MAX_SESSIONS` sessions already
    TooManySessions,
    /// A request field holds a value the request cannot take
    BadRequest(&'static str),
}

impl AgentError {
    pub fn message(self) -> &'static str {
        match self {
            AgentError::NoSession => "no such agent session on this page",
            AgentError::Ref(e) => e.message(),
            AgentError::TooFarBehind => "too far behind: take a new snapshot",
            AgentError::TooManySessions => "too many agent sessions on this page",
            AgentError::BadRequest(m) => m,
        }
    }
}

/// Which view to read.
#[derive(Clone, Debug)]
pub enum ViewKind {
    Outline,
    Snapshot,
    Markdown,
    Find(Query),
}
