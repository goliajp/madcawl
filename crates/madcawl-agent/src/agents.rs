//! The agents attached to one page: their sessions, each with its ring
//! of changes, and the semantic tree they share. With no session there
//! is no tree, no change record in the DOM, nothing allocated.

use std::sync::atomic::{AtomicU32, Ordering};

use madcawl_dom::{Dom, NodeId};

use crate::refs::{Ref, RefError};
pub use crate::request::{AgentError, ViewKind};
use crate::ring::{Entry, Ring};
use crate::sync::{Delta, live};
use crate::tree::{Inputs, SemTree};
use crate::view::{Scope, View, ViewOpts};

pub type SessionId = u32;

/// Changes a session's ring holds before a diff from before them is
/// refused.
pub const RING_CAP: usize = 4096;

/// Sessions one page takes at most.
pub const MAX_SESSIONS: usize = 64;

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
    /// Where page generations come from when they are shared with other
    /// pages; else they count up from this page's own
    gens: Option<&'static AtomicU32>,
    seq: u64,
    deltas: Vec<(u32, u32, Delta)>,
    /// Trees are built only for the page outline until more is asked
    defer: bool,
}

impl Agents {
    pub fn new() -> Self {
        Agents::default()
    }

    /// Agents whose page generations are drawn from `gens`, which every
    /// page of a host shares: a reference handed out on one page is then
    /// stale on every other, not just on this page's later documents.
    pub fn sharing_generations(gens: &'static AtomicU32) -> Self {
        Agents { gens: Some(gens), ..Agents::default() }
    }

    /// Agents that build a document's tree only as far as the page
    /// outline reads it, and the rest on the first request for more
    /// (`partial.rs`); every answer is the one a whole tree gives.
    pub fn deferring(self) -> Self {
        Agents {
            defer: true,
            ..self
        }
    }

    fn next_gen(&mut self) -> u32 {
        self.page_gen = match self.gens {
            Some(g) => g.fetch_add(1, Ordering::Relaxed).wrapping_add(1),
            None => self.page_gen + 1,
        };
        self.page_gen
    }

    /// A new session; the first builds the semantic tree.
    pub fn attach(&mut self, dom: &mut Dom, inputs: Inputs) -> Result<SessionId, AgentError> {
        if self.sessions.len() >= MAX_SESSIONS {
            return Err(AgentError::TooManySessions);
        }
        if self.tree.is_none() {
            let g = self.next_gen();
            self.tree = Some(self.build(dom, inputs, g, false));
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
        self.rebuild(dom, inputs, false);
    }

    fn build(&self, dom: &mut Dom, inputs: Inputs, g: u32, whole: bool) -> SemTree {
        match self.defer && !whole {
            true => SemTree::build_outline(dom, inputs, g),
            false => SemTree::build(dom, inputs, g),
        }
    }

    /// A tree built for the outline made whole when `whole` asks more
    /// than the page outline, or read again whole under a new page
    /// generation when what it was built from changed (`partial.rs`).
    pub(crate) fn settle(&mut self, dom: &mut Dom, inputs: Inputs, whole: bool) {
        if self
            .tree
            .as_mut()
            .is_some_and(|t| !t.settle(dom, inputs, whole))
        {
            self.rebuild(dom, inputs, true);
        }
    }

    /// A new tree under a new page generation: every reference handed out
    /// goes stale, every session's ring starts again.
    fn rebuild(&mut self, dom: &mut Dom, inputs: Inputs, whole: bool) {
        let g = self.next_gen();
        self.tree = Some(self.build(dom, inputs, g, whole));
        self.seq += 1;
        for s in &mut self.sessions {
            s.ring = Ring::new(RING_CAP, self.seq);
        }
    }

    pub fn is_attached(&self) -> bool {
        self.tree.is_some()
    }

    /// The tree, once it is whole.
    pub fn tree(&self) -> Option<&SemTree> {
        self.tree.as_ref().filter(|t| !t.is_partial())
    }

    pub(crate) fn tree_mut(&mut self) -> Option<&mut SemTree> {
        self.tree.as_mut().filter(|t| !t.is_partial())
    }

    /// The page generation of the references handed out; 0 with no session.
    pub fn page_gen(&self) -> u32 {
        self.tree.as_ref().map_or(0, SemTree::page_gen)
    }

    pub(crate) fn has_session(&self, id: SessionId) -> bool {
        self.sessions.iter().any(|s| s.id == id)
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
        self.settle(dom, inputs, true);
        let Some(t) = self.tree.as_mut() else {
            return self.seq;
        };
        self.deltas.clear();
        if !t.sync(dom, inputs, &mut self.deltas) {
            // the DOM's record overflowed: freed slots may have gone unseen
            self.rebuild(dom, inputs, true);
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
        let page_outline = matches!(kind, ViewKind::Outline) && opts.scope == Scope::Page;
        self.settle(dom, inputs, !page_outline);
        let seq = match self.tree.as_ref().is_some_and(SemTree::is_partial) {
            true => self.seq,
            false => self.sync(dom, inputs),
        };
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
        if rec.generation != r.slot_gen || !live(dom, n) || !dom.in_main_tree(n) {
            return Err(stale);
        }
        Ok(n)
    }
}
