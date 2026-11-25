//! What an agent reads of a page: the semantic tree beside the DOM (a
//! 16-byte record per node — role, name, states, value, visibility, slot
//! generation — and a pool of the strings they point at), element
//! references, the outline, snapshot, Markdown and find views with their
//! scopes and token budgets, diffs from a session's bounded ring of
//! changes, the envelope page content is handed over in, and the frames
//! of the agent protocol.
//!
//! None of it exists until an agent attaches: `Agents` holds no tree,
//! the DOM records no changes, nothing is allocated. The first session
//! builds the tree whole; after that it follows the DOM's change record
//! and recomputes only what a change can reach; the last session to go
//! releases it.
//!
//! Roles follow HTML-AAM with the ARIA `role` attribute's override,
//! names the accname algorithm. Shadow trees and frames are not read:
//! the DOM has neither yet.

mod agents;
mod cx;
mod diff;
mod envelope;
mod frame;
mod geom;
mod html_role;
mod html_table;
mod index;
mod name;
mod native;
mod pool;
mod protocol;
mod record;
mod refs;
mod ring;
mod role;
mod states;
mod sync;
mod text;
mod tree;
mod view;

pub use agents::{AgentError, Agents, MAX_SESSIONS, RING_CAP, SessionId, ViewKind};
pub use envelope::Envelope;
pub use frame::{Frame, FrameError, MAX_FIELDS, MAX_FRAME};
pub use geom::{Geometry, Place, Rect};
pub use name::Source;
pub use protocol::{kind, read_request, tag};
pub use record::Record;
pub use refs::{Ref, RefError, push_page_text, push_quoted};
pub use role::Role;
pub use sync::Delta;
pub use text::estimate_tokens;
pub use tree::{Inputs, MUTATION_CAP, SemTree};
pub use view::{Query, Scope, View, ViewOpts};

/// The record of a node and its name, for checking a tree against
/// expectations, such as a conformance suite's.
pub fn role_and_name(t: &SemTree, n: madcawl_dom::NodeId) -> Option<(Role, String)> {
    t.record(n).map(|r| (r.role, t.name(r).to_owned()))
}

#[cfg(test)]
mod sync_tests;
