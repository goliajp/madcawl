//! The DOM: nodes in one arena of 64-byte slots, attributes in a heap
//! beside it, and the tree operations that keep every node's reference
//! count by one set of rules.
//!
//! An arena is reserved once (2^24 slots, 1 GiB of address space, no
//! memory until used) and never grows or moves, so a node's address
//! never changes and index and address convert by a shift. Past its
//! node limit the DOM refuses to create nodes; past its byte limit it
//! cuts strings short. Either way the parser stops at its next token.
//!
//! A node's count is the number of these edges that point at it: its
//! parent holds it, each child holds it, each node holds its node
//! document, a document holds its template contents owner, a template
//! holds its contents, a script wrapper holds its node, and so does each
//! host handle (`HeldNode`). Siblings do not count. `Dom::audit`
//! recomputes them all.
//!
//! Nothing is freed while a parser builds into the DOM: the parser holds
//! handles no count records. `Dom::reclaim` frees what has left every
//! tree once it is done. Once script or the host holds nodes by count
//! (a wrapper, a `HeldNode`, suspects handed out), they are freed by
//! count instead, from then on (`host.rs`): a node whose count reaches
//! 0 is freed by `Dom::free_dead`, and the cycles counts cannot free
//! are the script engine's collector's, through `Dom::host_edges` and
//! `Dom::unlink_node`.

mod arena;
mod attrs;
mod audit;
mod classes;
mod dat;
mod dom;
mod edit;
mod held;
mod host;
mod limits;
mod node;
mod observe;
mod reclaim;
mod select;
mod select_cache;
mod serialize;
mod short;
mod sink;
mod sys;
mod text;
mod tree;

pub use audit::Audit;
pub use dom::{Attr, Dom, HEADROOM, Limits, NodeId, Refused};
pub use held::HeldNode;
pub use host::Edge;
pub use node::NodeKind;
pub use observe::Mutation;
