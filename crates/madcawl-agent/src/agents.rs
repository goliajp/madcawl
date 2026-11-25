//! The agents attached to one page: their sessions, each with its ring
//! of changes, and the semantic tree they share. With no session there
//! is no tree, no change record in the DOM, nothing allocated.

use madcawl_dom::{Dom, NodeId};

use crate::refs::{Ref, RefError};
use crate::ring::{Entry, Ring};
use crate::sync::{Delta, live};
use crate::tree::{Inputs, SemTree};
use crate::view::{Query, View, ViewOpts};

pub type SessionId = u32;

/// Changes a session's ring holds before a diff from before them is
/// refused.
pub const RING_CAP: usize = 4096;

/// Sessions one page takes at most.
pub const MAX_SESSIONS: usize = 64;

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

struct Session {
    id: SessionId,
    ring: Ring,
}

#[derive(Default)]
pub struct Agents {
    tree: Option<SemTree>,
    sessions: Vec<Session>,
    next: SessionId,
    page_gen: u32,
    seq: u64,
    deltas: Vec<(u32, u32, Delta)>,
}

impl Agents {
    pub fn new() -> Self {
        Agents::default()
    }

    /// A new session; the first builds the semantic tree.
    pub fn attach(&mut self, dom: &mut Dom, inputs: Inputs) -> Result<SessionId, AgentError> {
        if self.sessions.len() >= MAX_SESSIONS {
            return Err(AgentError::TooManySessions);
        }
        if self.tree.is_none() {
            self.page_gen += 1;
            self.tree = Some(SemTree::build(dom, inputs, self.page_gen));
        }
        self.next += 1;
        self.sessions.push(Session {
            id: self.next,
            ring: Ring::new(RING_CAP, self.seq),
        });
        Ok(self.next)
    }

    /// Ends a session; the last one releases the tree and stops the DOM
    /// recording changes.
    pub fn detach(&mut self, id: SessionId, dom: &mut Dom) {
        self.sessions.retain(|s| s.id != id);
        if self.sessions.is_empty()
            && let Some(t) = self.tree.take()
        {
            t.release(dom);
            self.deltas = Vec::new();
            self.sessions = Vec::new();
        }
    }

    /// The page navigated to a new document: references to the old one go
    /// stale, the tree is built again for `dom`, sessions stay.
    pub fn navigated(&mut self, dom: &mut Dom, inputs: Inputs) {
        if self.tree.take().is_none() {
            return;
        }
        self.rebuild(dom, inputs);
    }

    /// A new tree under a new page generation: every reference handed out
    /// goes stale, every session's ring starts again.
    fn rebuild(&mut self, dom: &mut Dom, inputs: Inputs) {
        self.page_gen += 1;
        self.tree = Some(SemTree::build(dom, inputs, self.page_gen));
        self.seq += 1;
        for s in &mut self.sessions {
            s.ring = Ring::new(RING_CAP, self.seq);
        }
    }

    pub fn is_attached(&self) -> bool {
        self.tree.is_some()
    }

    pub fn tree(&self) -> Option<&SemTree> {
        self.tree.as_ref()
    }

    /// The bytes the agents' structures hold: 0 with no session.
    pub fn bytes(&self) -> usize {
        let rings: usize = self.sessions.iter().map(|s| s.ring.bytes()).sum();
        self.tree.as_ref().map_or(0, SemTree::bytes)
            + rings
            + self.sessions.capacity() * size_of::<Session>()
            + self.deltas.capacity() * size_of::<(u32, u32, Delta)>()
    }

    /// The sequence number the next view is stamped with.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Brings the tree up to date with the DOM and puts what changed into
    /// every session's ring; the sequence advances when anything did.
    pub fn sync(&mut self, dom: &mut Dom, inputs: Inputs) -> u64 {
        let Some(t) = self.tree.as_mut() else {
            return self.seq;
        };
        self.deltas.clear();
        if !t.sync(dom, inputs, &mut self.deltas) {
            // the DOM's record overflowed: freed slots may have gone unseen
            self.rebuild(dom, inputs);
            return self.seq;
        }
        if !self.deltas.is_empty() {
            self.seq += 1;
            for s in &mut self.sessions {
                for &(slot, generation, delta) in &self.deltas {
                    s.ring.push(Entry {
                        seq: self.seq,
                        slot,
                        generation,
                        delta,
                    });
                }
            }
        }
        self.seq
    }

    fn check(&self, id: SessionId) -> Result<&SemTree, AgentError> {
        if !self.sessions.iter().any(|s| s.id == id) {
            return Err(AgentError::NoSession);
        }
        self.tree.as_ref().ok_or(AgentError::NoSession)
    }

    /// A view, after bringing the tree up to date; with the sequence it
    /// shows the page at.
    pub fn view(
        &mut self,
        id: SessionId,
        dom: &mut Dom,
        inputs: Inputs,
        kind: &ViewKind,
        opts: ViewOpts,
    ) -> Result<(u64, View), AgentError> {
        self.check(id)?;
        let seq = self.sync(dom, inputs);
        let t = self.check(id)?;
        let v = match kind {
            ViewKind::Outline => t.outline(dom, inputs, opts),
            ViewKind::Snapshot => t.snapshot(dom, inputs, opts),
            ViewKind::Markdown => t.markdown(dom, inputs, opts),
            ViewKind::Find(q) => t.find(dom, inputs, opts, q),
        };
        Ok((seq, v))
    }

    /// Records extracted after bringing the tree up to date, written as
    /// `opts` asks; with the sequence they show the page at.
    pub fn extract(
        &mut self,
        id: SessionId,
        dom: &mut Dom,
        inputs: Inputs,
        opts: &crate::extract::ExtractOpts,
    ) -> Result<(u64, View), AgentError> {
        self.check(id)?;
        let seq = self.sync(dom, inputs);
        let t = self.check(id)?;
        Ok((seq, t.extract_view(dom, inputs, opts)))
    }

    /// What changed since the view stamped `since`, one line a node.
    pub fn diff(
        &mut self,
        id: SessionId,
        dom: &mut Dom,
        inputs: Inputs,
        since: u64,
    ) -> Result<(u64, View), AgentError> {
        self.check(id)?;
        let seq = self.sync(dom, inputs);
        let t = self.check(id)?;
        let s = self.sessions.iter().find(|s| s.id == id).expect("checked");
        let entries = s.ring.since(since).ok_or(AgentError::TooFarBehind)?;
        let text = crate::diff::write(t, dom, inputs, since, seq, entries);
        Ok((seq, crate::view::finish_text(text)))
    }

    /// The node a reference names, if it still does (the tree is brought
    /// up to date first, so a freed node's slot taken by another is seen).
    pub fn resolve(
        &mut self,
        id: SessionId,
        dom: &mut Dom,
        inputs: Inputs,
        r: &str,
    ) -> Result<NodeId, AgentError> {
        self.check(id)?;
        self.sync(dom, inputs);
        let t = self.check(id)?;
        let r = Ref::parse(r).map_err(AgentError::Ref)?;
        let stale = AgentError::Ref(RefError::Stale);
        if r.page_gen != t.page_gen() {
            return Err(stale);
        }
        let n = NodeId::from_index(r.index).ok_or(stale)?;
        let rec = t.record(n).ok_or(stale)?;
        if rec.generation != r.slot_gen || !live(dom, n) || !dom.is_connected(n) {
            return Err(stale);
        }
        Ok(n)
    }
}
