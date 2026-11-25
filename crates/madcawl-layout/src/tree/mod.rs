//! The box tree: what the styled DOM generates, in its own arena of 32-bit indices. Elements with
//! `display: none` generate nothing and `display: contents` ones generate no box of their own; a block
//! container's children are all block-level or all inline-level, inline runs among blocks being wrapped in
//! anonymous blocks, and a block inside an inline box splits the inline box around it.

mod build;
mod generated;
mod kind;

use madcawl_dom::NodeId;

pub use build::{MAX_DEPTH, build as build_tree};

/// No box.
pub const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A block-level block container
    Block,
    /// A block-level box wrapping inline content among blocks; its style is its parent's
    AnonBlock,
    /// An atomic inline: a block container laid out as one unbreakable piece of a line
    InlineBlock,
    /// An inline box; a split one has a box for each piece, all for the same element
    Inline,
    /// The text of a text node
    Text,
    /// The text a `::before` or `::after` generates, by its index among the tree's generated texts
    Generated(u32),
    /// `<br>`
    Break,
    /// A replaced element: its contents are not laid out here
    Replaced { inline: bool },
}

impl Kind {
    pub fn is_inline_level(self) -> bool {
        matches!(
            self,
            Kind::InlineBlock
                | Kind::Inline
                | Kind::Text
                | Kind::Generated(_)
                | Kind::Break
                | Kind::Replaced { inline: true }
        )
    }

    /// Whether its children are laid out by this crate's block or inline layout.
    pub fn is_block_container(self) -> bool {
        matches!(self, Kind::Block | Kind::AnonBlock | Kind::InlineBlock)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BoxNode {
    pub kind: Kind,
    /// The element or text node it is for; an anonymous box has its parent's element
    pub node: NodeId,
    pub parent: u32,
    pub first: u32,
    pub last: u32,
    pub next: u32,
    /// 1 for a box of the element's `::before` and the text in it, 2 for `::after`, else 0
    pub pseudo: u8,
}

/// The boxes of a document; box 0 is the root element's, if it has one.
#[derive(Default)]
pub struct BoxTree {
    pub(crate) boxes: Vec<BoxNode>,
    /// Whether a block container's children are inline-level, by box
    pub(crate) inline_children: Vec<bool>,
    /// Blocks that split inline boxes, with each element whose box they split: its client rectangles
    /// include theirs
    pub(crate) hoisted: Vec<(u32, NodeId)>,
    /// The anonymous table boxes and what each is
    pub(crate) table_roles: crate::table::Roles,
    /// The texts of `Kind::Generated` boxes
    pub(crate) generated: Vec<Box<[u16]>>,
    /// Some box is `position: sticky`
    pub(crate) sticky: bool,
}

impl BoxTree {
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    pub fn get(&self, b: u32) -> &BoxNode {
        &self.boxes[b as usize]
    }

    pub fn root(&self) -> Option<u32> {
        (!self.boxes.is_empty()).then_some(0)
    }

    pub fn children(&self, b: u32) -> impl Iterator<Item = u32> + '_ {
        let first = self.boxes[b as usize].first;
        std::iter::successors((first != NONE).then_some(first), |&c| {
            let n = self.boxes[c as usize].next;
            (n != NONE).then_some(n)
        })
    }

    /// Whether a block container lays out its children in lines.
    pub fn has_inline_children(&self, b: u32) -> bool {
        self.inline_children[b as usize]
    }

    /// The depth of the deepest box, the root being 1.
    pub fn depth(&self) -> usize {
        let Some(root) = self.root() else { return 0 };
        let (mut stack, mut max) = (vec![(root, 1usize)], 0);
        while let Some((b, d)) = stack.pop() {
            max = max.max(d);
            stack.extend(self.children(b).map(|c| (c, d + 1)));
        }
        max
    }
}
