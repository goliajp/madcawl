//! The DOM of one agent, its limits, and reading it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::num::NonZeroU32;

use madcawl_html::{AttrNamespace, Namespace, QuirksMode};
use madcawl_str::{Atom, DomString, HeldAtoms};

use crate::arena::{Arena, SLOTS};
use crate::attrs::AttrHeap;
use crate::classes::Tables;
pub use crate::limits::{HEADROOM, Limits, Refused};
use crate::node::{
    IN_DOCUMENT, INERT, KIND_MASK, NS_MASK, NS_SHIFT, Node, NodeKind, QUIRKS_MASK, QUIRKS_SHIFT,
};
use crate::short::ShortStrings;
use crate::text::Tail;

/// A node of one `Dom`: its slot index, never 0.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct NodeId(NonZeroU32);

impl NodeId {
    pub(crate) fn of(i: u32) -> Option<NodeId> {
        NonZeroU32::new(i).map(NodeId)
    }

    pub(crate) fn at(i: u32) -> NodeId {
        NodeId(NonZeroU32::new(i).expect("a node index is never 0"))
    }

    /// The node of slot `i`, 0 being none; whether the slot holds a live
    /// node is for `Dom::node_at` to say.
    pub fn from_index(i: u32) -> Option<NodeId> {
        NodeId::of(i)
    }

    /// The slot index, dense from 1: what side tables are indexed by.
    pub fn index(self) -> u32 {
        self.0.get()
    }
}

#[derive(Default)]
pub(crate) struct Rare {
    pub contents: u32,
    pub inert: u32,
    pub form: u32,
    pub doctype: Option<Box<[DomString; 3]>>,
    /// A select's first option and first selectedcontent below it, when
    /// `select_known`; otherwise found again by a scan.
    pub select_option: u32,
    pub select_content: u32,
    pub select_known: bool,
}

pub struct Dom {
    pub(crate) arena: Arena,
    pub(crate) attrs: AttrHeap,
    pub(crate) tables: Tables,
    /// Entry 0 is never used.
    pub(crate) rare: Vec<Rare>,
    /// Entries of freed nodes, used again.
    pub(crate) rare_free: Vec<u32>,
    /// Short whitespace strings, shared by the text nodes that are only
    /// whitespace.
    pub(crate) spaces: Vec<DomString>,
    /// Short attribute values and texts, shared.
    pub(crate) short: ShortStrings,
    pub(crate) tail: RefCell<Option<Tail>>,
    /// The dynamic atoms names use.
    pub(crate) atoms: HeldAtoms,
    pub(crate) doc: u32,
    pub(crate) limits: Limits,
    pub(crate) bytes: usize,
    pub(crate) cut: Option<Refused>,
    pub(crate) stopped: bool,
    /// Nodes whose count reached 0, for `free_dead`.
    pub(crate) zero: Vec<u32>,
    /// Nodes whose count dropped and stayed above 0 outside every
    /// document, each once (`SUSPECTED`), for `take_suspects`.
    pub(crate) suspects: Vec<u32>,
    /// The counts host handles hold, by node index (`HeldNode`).
    pub(crate) held: HashMap<u32, u32>,
    /// Script or the host has held nodes by count: `reclaim`, which frees
    /// by reach and tells no one, may no longer run.
    pub(crate) by_count: bool,
    /// The change record of an observer, when one asked (`observe.rs`)
    pub(crate) observer: Option<Box<crate::observe::Observer>>,
}

/// An attribute as stored.
pub struct Attr<'a> {
    pub ns: AttrNamespace,
    pub prefix: Option<Atom>,
    pub local: Atom,
    pub value: &'a DomString,
}

impl Dom {
    pub fn new() -> Self {
        Self::with_limits(Limits::default())
    }

    pub fn with_limits(limits: Limits) -> Self {
        assert!(
            (2 * HEADROOM..SLOTS as u32).contains(&limits.nodes),
            "a node limit from {} to 2^24 - 1",
            2 * HEADROOM
        );
        // a string under the byte limit is under the string length limit
        assert!(
            limits.bytes.max(limits.tables) <= 1 << 30,
            "limits up to 1 GiB"
        );
        let mut dom = Dom {
            arena: Arena::new(),
            attrs: AttrHeap::new(),
            tables: Tables::new(limits.tables),
            rare: vec![Rare::default()],
            rare_free: Vec::new(),
            spaces: Vec::new(),
            short: ShortStrings::default(),
            tail: RefCell::new(None),
            atoms: HeldAtoms::new(),
            doc: 0,
            limits,
            bytes: 0,
            cut: None,
            stopped: false,
            zero: Vec::new(),
            suspects: Vec::new(),
            held: HashMap::new(),
            by_count: false,
            observer: None,
        };
        dom.doc = dom.new_document(false).expect("room for the document");
        dom
    }

    pub(crate) fn n(&self, i: u32) -> &Node {
        self.arena.get(i)
    }

    pub(crate) fn m(&mut self, i: u32) -> &mut Node {
        self.arena.get_mut(i)
    }

    pub(crate) fn kind_of(&self, i: u32) -> NodeKind {
        self.n(i).kind().expect("a live node")
    }

    pub fn document(&self) -> NodeId {
        NodeId::at(self.doc)
    }

    pub fn kind(&self, n: NodeId) -> NodeKind {
        self.kind_of(n.index())
    }

    pub fn parent(&self, n: NodeId) -> Option<NodeId> {
        NodeId::of(self.n(n.index()).parent)
    }

    pub fn next_sibling(&self, n: NodeId) -> Option<NodeId> {
        NodeId::of(self.n(n.index()).next)
    }

    pub fn previous_sibling(&self, n: NodeId) -> Option<NodeId> {
        NodeId::of(self.n(n.index()).prev)
    }

    pub fn first_child(&self, n: NodeId) -> Option<NodeId> {
        let i = n.index();
        NodeId::of(if self.kind_of(i).is_container() {
            self.n(i).first()
        } else {
            0
        })
    }

    pub fn last_child(&self, n: NodeId) -> Option<NodeId> {
        let i = n.index();
        NodeId::of(if self.kind_of(i).is_container() {
            self.n(i).last()
        } else {
            0
        })
    }

    pub fn children(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.first_child(n), |&c| self.next_sibling(c))
    }

    /// The nodes below `root`, in tree order; template contents are
    /// not below their template.
    pub fn descendants(&self, root: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let r = root.index();
        std::iter::successors(NodeId::of(self.next_in(r, r)), move |&c| {
            NodeId::of(self.next_in(c.index(), r))
        })
    }

    pub fn namespace(&self, n: NodeId) -> Option<Namespace> {
        let f = self.n(n.index()).flags;
        let ns = match (f & NS_MASK) >> NS_SHIFT {
            0 => Namespace::Html,
            1 => Namespace::Svg,
            _ => Namespace::MathMl,
        };
        (f & KIND_MASK == NodeKind::Element as u32).then_some(ns)
    }

    /// An element's local name.
    pub fn local_name(&self, n: NodeId) -> Option<Atom> {
        (self.kind(n) == NodeKind::Element).then(|| Atom::from_index(self.n(n.index()).name))
    }

    pub(crate) fn is_html(&self, i: u32, name: Atom) -> bool {
        let n = self.n(i);
        n.flags & (KIND_MASK | NS_MASK) == NodeKind::Element as u32 && n.name == name.index()
    }

    pub fn attributes(&self, n: NodeId) -> impl Iterator<Item = Attr<'_>> + '_ {
        let b = self.n(n.index()).attrs;
        let list = if b == 0 { &[][..] } else { self.attrs.attrs(b) };
        list.iter().map(|u| Attr {
            ns: u.ns(),
            prefix: u.prefix(),
            local: u.local(),
            value: u.value(),
        })
    }

    /// The value of the attribute with no namespace named `local`.
    pub fn attribute(&self, n: NodeId, local: Atom) -> Option<&DomString> {
        self.attributes(n)
            .find(|a| a.ns == AttrNamespace::None && a.local == local)
            .map(|a| a.value)
    }

    pub(crate) fn has_attr(&self, i: u32, local: Atom) -> bool {
        self.attribute(NodeId::at(i), local).is_some()
    }

    /// The data of a text, comment or processing instruction node.
    pub fn text(&self, n: NodeId) -> Option<&DomString> {
        let i = n.index();
        self.kind_of(i).is_char_data().then(|| self.char_data(i))
    }

    pub fn pi_target(&self, n: NodeId) -> Option<Atom> {
        (self.kind(n) == NodeKind::ProcessingInstruction)
            .then(|| Atom::from_index(self.n(n.index()).name))
    }

    /// A DOCTYPE's name, public id and system id.
    pub fn doctype(&self, n: NodeId) -> Option<&[DomString; 3]> {
        let r = self.n(n.index()).rare;
        self.rare[r as usize].doctype.as_deref().filter(|_| r != 0)
    }

    /// A template element's contents.
    pub fn contents(&self, n: NodeId) -> Option<NodeId> {
        NodeId::of(self.rare[self.n(n.index()).rare as usize].contents)
    }

    pub fn form_owner(&self, n: NodeId) -> Option<NodeId> {
        NodeId::of(self.rare[self.n(n.index()).rare as usize].form)
    }

    pub fn quirks_mode(&self) -> QuirksMode {
        match (self.n(self.doc).flags & QUIRKS_MASK) >> QUIRKS_SHIFT {
            0 => QuirksMode::NoQuirks,
            1 => QuirksMode::LimitedQuirks,
            _ => QuirksMode::Quirks,
        }
    }

    pub fn is_connected(&self, n: NodeId) -> bool {
        self.n(n.index()).flags & IN_DOCUMENT != 0
    }

    pub fn is_inert_document(&self, n: NodeId) -> bool {
        self.kind(n) == NodeKind::Document && self.n(n.index()).flags & INERT != 0
    }

    /// The element's ID.
    pub fn element_id(&self, n: NodeId) -> Option<Atom> {
        let b = self.n(n.index()).attrs;
        if b == 0 { None } else { self.attrs.id(b) }
    }

    /// The element's class list number and classes; equal numbers are
    /// equal lists. 0 is the empty list.
    pub fn class_list(&self, n: NodeId) -> (u32, &[Atom]) {
        let b = self.n(n.index()).attrs;
        let k = if b == 0 { 0 } else { self.attrs.classes(b) };
        (k, self.tables.list(k))
    }

    /// The node's reference count.
    pub fn ref_count(&self, n: NodeId) -> u32 {
        self.n(n.index()).count()
    }

    pub fn node_count(&self) -> u32 {
        self.arena.count()
    }

    /// The bytes counted against `Limits::bytes`.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The node's address, fixed from creation to release.
    pub fn address(&self, n: NodeId) -> usize {
        self.arena.addr(n.index())
    }

    /// The live node at `addr`, if it is one of this DOM's.
    pub fn node_at(&self, addr: usize) -> Option<NodeId> {
        let ours = Arena::base_of(addr) == self.arena.addr(0) && addr.is_multiple_of(64);
        let i = if ours { self.arena.index(addr) } else { 0 };
        self.arena.is_live(i).then(|| NodeId::at(i))
    }

    /// Which limit the DOM has reached, if one.
    pub fn refused(&self) -> Option<Refused> {
        if self.cut.is_some() {
            self.cut
        } else if self.arena.count() + HEADROOM > self.limits.nodes {
            Some(Refused::Nodes)
        } else if self.bytes >= self.limits.bytes {
            Some(Refused::Bytes)
        } else {
            None
        }
    }

    /// The parser stopped because the DOM was full.
    pub fn parser_stopped(&self) -> bool {
        self.stopped
    }
}

impl Default for Dom {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Dom {
    fn drop(&mut self) {
        for i in 1..self.arena.high() {
            if self.arena.is_live(i) {
                self.release_contents(i);
            }
        }
    }
}
