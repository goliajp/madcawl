//! How much one DOM may hold.

use crate::arena::SLOTS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most nodes the DOM holds.
    pub nodes: u32,
    /// The most bytes of text, comments, attribute values and attribute
    /// slots (16 bytes each) the parser may hand the DOM.
    pub bytes: usize,
}

/// The parser stops while at least this many nodes are left, because a
/// token it has started cannot be refused a node. One token creates at
/// most: the table text pending from the previous token, flushed with
/// foster parenting (reopening up to 256 formatting elements, the list's
/// limit, plus the text node), then the token's own reopening (256 more)
/// and its element, up to 32 elements from the adoption agency (8 outer
/// rounds of 3 inner clones plus the formatting element), and at most 8
/// implied elements (html, head, body, colgroup, tbody, tr and the
/// like): 2 * 257 + 32 + 8 = 554. 1024 leaves the rest as margin.
pub const HEADROOM: u32 = 1024;

impl Default for Limits {
    fn default() -> Self {
        Limits {
            nodes: SLOTS as u32 - 1,
            bytes: 1 << 30,
        }
    }
}

/// Which limit made the DOM refuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    Nodes,
    Bytes,
}
