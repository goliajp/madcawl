//! The 64-byte node: one slot of the arena, one cache line.

use std::mem::{align_of, offset_of, size_of};

use madcawl_abi as abi;

/// Every kind of node has this layout. Fields are indices into the
/// arena (0 is none) unless said otherwise.
#[repr(C, align(64))]
pub(crate) struct Node {
    /// The engine's object header: count, type tag, flags. Only the
    /// count is written, through `madcawl-abi`; the tag and flag halves
    /// stay zero until the host type range is agreed with the engine.
    pub header: u64,
    pub parent: u32,
    pub prev: u32,
    pub next: u32,
    /// madcawl's own bits: `KIND_*`, `NS_*` and the rest below.
    pub flags: u32,
    /// A container's first child (low half) and last child (high half),
    /// or a character data node's `DomString` block pointer.
    pub data: u64,
    /// An element's local name, a processing instruction's target: an
    /// atom index.
    pub name: u32,
    /// The node document.
    pub owner: u32,
    /// The attribute block in the attribute heap.
    pub attrs: u32,
    /// The computed style, once there is a style system.
    pub style: u32,
    /// The script wrapper's pointer, which holds the node (an edge
    /// counted here); the low bit set when the wrapper is preserved, an
    /// edge counted on the wrapper.
    pub wrapper: u64,
    /// The rare data entry.
    pub rare: u32,
    /// The tree scope: the node document until there are shadow roots.
    pub scope: u32,
}

const _: () = assert!(size_of::<Node>() == 64);
const _: () = assert!(align_of::<Node>() == 64);
const _: () = assert!(offset_of!(Node, header) == abi::REFCOUNT_OFF);
const _: () = assert!(offset_of!(Node, header) == 0);
const _: () = assert!(offset_of!(Node, parent) == 8);
const _: () = assert!(offset_of!(Node, prev) == 12);
const _: () = assert!(offset_of!(Node, next) == 16);
const _: () = assert!(offset_of!(Node, flags) == 20);
const _: () = assert!(offset_of!(Node, data) == 24);
const _: () = assert!(offset_of!(Node, name) == 32);
const _: () = assert!(offset_of!(Node, owner) == 36);
const _: () = assert!(offset_of!(Node, attrs) == 40);
const _: () = assert!(offset_of!(Node, style) == 44);
const _: () = assert!(offset_of!(Node, wrapper) == 48);
const _: () = assert!(offset_of!(Node, rare) == 56);
const _: () = assert!(offset_of!(Node, scope) == 60);
// the data word holds a block pointer
const _: () = assert!(size_of::<usize>() == 8);

/// The kind, in the low three bits; 0 is a free slot.
pub(crate) const KIND_MASK: u32 = 0b111;
/// An element's namespace: 0 HTML, 1 SVG, 2 MathML.
pub(crate) const NS_SHIFT: u32 = 3;
pub(crate) const NS_MASK: u32 = 0b11 << NS_SHIFT;
/// The node's root is a document.
pub(crate) const IN_DOCUMENT: u32 = 1 << 5;
/// The node changed since its style was last computed.
pub(crate) const STYLE_DIRTY: u32 = 1 << 6;
/// The node is in the suspects list, which then holds it once.
pub(crate) const SUSPECTED: u32 = 1 << 7;
pub(crate) const HAS_ID: u32 = 1 << 8;
pub(crate) const HAS_CLASS: u32 = 1 << 9;
pub(crate) const HAS_STYLE: u32 = 1 << 10;
// bits 11 to 14 are kept for the states selectors match (hover, active,
// focus, focus-visible)
/// A document that is a template contents owner.
pub(crate) const INERT: u32 = 1 << 15;
/// A document's quirks mode: 0 no quirks, 1 limited, 2 quirks.
pub(crate) const QUIRKS_SHIFT: u32 = 16;
pub(crate) const QUIRKS_MASK: u32 = 0b11 << QUIRKS_SHIFT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum NodeKind {
    Document = 1,
    Doctype = 2,
    Element = 3,
    Text = 4,
    Comment = 5,
    ProcessingInstruction = 6,
    Fragment = 7,
}

impl NodeKind {
    pub(crate) fn of(flags: u32) -> Option<NodeKind> {
        Some(match flags & KIND_MASK {
            1 => NodeKind::Document,
            2 => NodeKind::Doctype,
            3 => NodeKind::Element,
            4 => NodeKind::Text,
            5 => NodeKind::Comment,
            6 => NodeKind::ProcessingInstruction,
            7 => NodeKind::Fragment,
            _ => return None,
        })
    }

    /// It has children: the data word holds its first and last child.
    pub fn is_container(self) -> bool {
        matches!(
            self,
            NodeKind::Document | NodeKind::Element | NodeKind::Fragment
        )
    }

    /// The data word holds a `DomString`.
    pub fn is_char_data(self) -> bool {
        matches!(
            self,
            NodeKind::Text | NodeKind::Comment | NodeKind::ProcessingInstruction
        )
    }
}

impl Node {
    pub(crate) fn new(kind: NodeKind, owner: u32) -> Node {
        Node {
            header: abi::with_refcount(0, 0),
            parent: 0,
            prev: 0,
            next: 0,
            flags: kind as u32 | STYLE_DIRTY,
            data: 0,
            name: 0,
            owner,
            attrs: 0,
            style: 0,
            wrapper: 0,
            rare: 0,
            scope: owner,
        }
    }

    pub(crate) fn kind(&self) -> Option<NodeKind> {
        NodeKind::of(self.flags)
    }

    pub(crate) fn count(&self) -> u32 {
        abi::refcount(self.header)
    }

    pub(crate) fn set_count(&mut self, n: u32) {
        self.header = abi::with_refcount(self.header, n);
    }

    pub(crate) fn first(&self) -> u32 {
        self.data as u32
    }

    pub(crate) fn last(&self) -> u32 {
        (self.data >> 32) as u32
    }

    pub(crate) fn set_first(&mut self, i: u32) {
        self.data = self.data & !0xFFFF_FFFF | i as u64;
    }

    pub(crate) fn set_last(&mut self, i: u32) {
        self.data = self.data & 0xFFFF_FFFF | (i as u64) << 32;
    }
}
