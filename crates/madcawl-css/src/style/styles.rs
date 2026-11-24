//! The computed values of a document's elements, and what the cascade
//! keeps beside them to restyle the document as it changes.

use std::collections::HashMap;

use madcawl_dom::{Dom, NodeId, NodeKind};

use super::computed::ComputedStyle;
use super::custom::{self, Memo};
use super::pathkey::PathKeys;
use super::rules;
use super::values::{ContentVisibility, Visibility};

/// The element or one of its ancestors is `display: none`.
pub(crate) const NOT_RENDERED: u8 = 1;
/// An ancestor is `content-visibility: hidden`, or otherwise does not
/// render what is in it: its contents are skipped.
pub(crate) const CONTENTS_SKIPPED: u8 = 2;
/// The element's own children are not rendered (`render.rs`).
pub(crate) const CHILDREN_HIDDEN: u8 = 4;

/// What the last run of the cascade did: elements styled, how many of
/// them by `PathKey`, and of those how many were computed again in full
/// to check, and how many of the checks found other values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StyleStats {
    pub elements: u64,
    pub hits: u64,
    pub verified: u64,
    pub mismatches: u64,
    pub first_mismatch: Option<NodeId>,
}

/// The computed values of a document's elements, by node.
#[derive(Default)]
pub struct Styles {
    /// By node index, 1 + the index in `table`; 0 for a node that has none
    pub(crate) of: Vec<u32>,
    pub(crate) flags: Vec<u8>,
    pub(crate) table: Vec<ComputedStyle>,
    pub(crate) interned: HashMap<ComputedStyle, u32>,
    /// By node index, the element's custom property set
    pub(crate) customs: Vec<u32>,
    pub(crate) sets: Vec<custom::CustomSet>,
    pub(crate) memo: Memo,
    /// The root element's font size, which `rem` is of
    pub(crate) root_font: f32,
    /// By node index, the element's `PathKey`
    pub(crate) keys: Vec<u32>,
    pub(crate) pk: PathKeys,
    /// The cascade these were made by, and the quirks mode of the
    /// document, which decides how ids and classes match
    pub(crate) generation: u64,
    pub(crate) quirks: bool,
    pub(crate) stats: StyleStats,
}

impl Styles {
    pub(crate) fn grow(&mut self, i: usize) {
        if self.of.len() <= i {
            let n = i + 1;
            self.of.resize(n, 0);
            self.flags.resize(n, 0);
            self.customs.resize(n, rules::NONE);
            self.keys.resize(n, 0);
        }
    }

    /// The place of `s` in the table, the same for equal values.
    pub(crate) fn intern(&mut self, s: ComputedStyle) -> u32 {
        if let Some(&k) = self.interned.get(&s) {
            return k;
        }
        let k = self.table.len() as u32;
        self.table.push(s.clone());
        self.interned.insert(s, k);
        k
    }

    /// What the last run of the cascade over these styles did.
    pub fn stats(&self) -> StyleStats {
        self.stats
    }

    /// How many `PathKey`s the document's elements have had.
    pub fn path_keys(&self) -> usize {
        self.pk.len()
    }

    /// The bytes the `PathKey` tables hold of style attributes, hints,
    /// names and signatures.
    pub fn path_key_bytes(&self) -> usize {
        self.pk.held()
    }

    /// The computed values of an element in the document.
    pub fn get(&self, n: NodeId) -> Option<&ComputedStyle> {
        let k = *self.of.get(n.index() as usize)?;
        (k != 0).then(|| &self.table[k as usize - 1])
    }

    /// The computed value of the custom property `name` (with its
    /// dashes) on an element.
    pub fn custom_property(&self, n: NodeId, name: &str) -> Option<&str> {
        let k = *self.customs.get(n.index() as usize)?;
        custom::lookup(&self.sets, k, name)
    }

    /// Every custom property of an element and its computed value, by
    /// name.
    pub fn custom_properties(&self, n: NodeId) -> Vec<(&str, &str)> {
        let k = self
            .customs
            .get(n.index() as usize)
            .copied()
            .unwrap_or(rules::NONE);
        custom::all(&self.sets, k)
    }

    /// How many different sets of computed values the elements have.
    pub fn distinct(&self) -> usize {
        self.table.len()
    }

    fn flags_of(&self, dom: &Dom, n: NodeId) -> Option<(u8, &ComputedStyle)> {
        let el = match dom.kind(n) {
            NodeKind::Element => n,
            _ => dom
                .parent(n)
                .filter(|&p| dom.kind(p) == NodeKind::Element)?,
        };
        let s = self.get(el)?;
        let f = self.flags[el.index() as usize];
        let skipped = el != n
            && (s.content_visibility == ContentVisibility::Hidden && s.display.can_contain_size()
                || f & CHILDREN_HIDDEN != 0);
        Some((
            f & !CHILDREN_HIDDEN | if skipped { CONTENTS_SKIPPED } else { 0 },
            s,
        ))
    }

    /// Whether the node makes boxes: an element or text in the document
    /// with no `display: none` on it or above it (nor `display:
    /// contents` on a replaced element or form control), no
    /// `content-visibility: hidden` above it on a box that can take size
    /// containment, and not in what HTML and
    /// SVG never render where it is (a closed `<details>` but its
    /// summary, the fallback content of `<canvas>`, `<video>`,
    /// `<audio>` and `<iframe>`, SVG `<defs>`, `<title>` and the like).
    /// A text node goes by its parent element.
    pub fn is_rendered(&self, dom: &Dom, n: NodeId) -> bool {
        self.flags_of(dom, n).is_some_and(|(f, _)| f == 0)
    }

    /// Rendered, and `visibility: visible`.
    pub fn is_visible(&self, dom: &Dom, n: NodeId) -> bool {
        self.flags_of(dom, n)
            .is_some_and(|(f, s)| f == 0 && s.visibility == Visibility::Visible)
    }
}
