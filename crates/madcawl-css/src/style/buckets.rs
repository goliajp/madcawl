//! Rules by the most particular simple selector of their rightmost
//! compound: an id, a class, a local name, `:root`, an attribute name,
//! or none. An element is tried against the buckets of its id, its
//! classes, its name, `:root` if it is the root, and the names of its
//! attributes, and the rules in none.

use std::collections::HashMap;

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_str::atom;

use crate::selector::{Component, PseudoElement};

use super::caseless::{caseless_hash, words};
use super::compute::Run;
use super::decls::What;
use super::keys::{Key, Namer};
use super::props::Prop;
use super::rules::{Rules, Sel};

/// Ids and classes are keyed by their atom, or in quirks mode, where
/// they match ASCII case-insensitively, by `caseless_hash` of their
/// text.
#[derive(Default)]
pub(crate) struct Buckets {
    quirks: bool,
    id: HashMap<u64, Vec<u32>>,
    class: HashMap<u64, Vec<u32>>,
    local: HashMap<u32, Vec<u32>>,
    /// Rules with `:root` and no id, class or name: they match the root element only
    root: Vec<u32>,
    /// Rules with an attribute selector and no id, class, name or `:root`, by the attribute's name as
    /// written and lowercase: an element they match has an attribute of one of the two names
    attr: HashMap<u32, Vec<u32>>,
    any: Vec<u32>,
    /// By rule, for the pseudo-element buckets, 1 for a `::before` rule and 2 for an `::after` one
    pub kinds: Vec<u8>,
}

/// Whether the block of rule `i` declares `content`.
pub fn declares_content(rules: &Rules<'_>, i: usize) -> bool {
    let block = rules.entries[i].block;
    rules.blocks[block as usize]
        .get()
        .iter()
        .any(|d| d.what == What::Prop(Prop::Content))
}

impl Buckets {
    /// The buckets of the rules for elements, or with `pseudo` of those for `::before` and `::after`; of
    /// those `keep` gives true for.
    pub fn build(
        rules: &Rules<'_>,
        quirks: bool,
        pseudo: bool,
        keep: impl Fn(usize) -> bool,
    ) -> Buckets {
        let mut b = Buckets {
            quirks,
            ..Buckets::default()
        };
        for (i, e) in rules.entries.iter().enumerate() {
            let n = Namer {
                data: &rules.sheets[e.sheet as usize],
                quirks,
            };
            let comps = match e.sel {
                Sel::Complex(c) => c.comps,
                Sel::Parent => &[],
            };
            let generated = comps
                .iter()
                .take_while(|c| !matches!(c, Component::Combinator(_)))
                .find_map(|c| match c {
                    Component::PseudoElement(
                        e @ (PseudoElement::Before | PseudoElement::After),
                    ) => Some(*e),
                    _ => None,
                });
            if pseudo && let Some(g) = generated {
                b.kinds.resize(rules.entries.len(), 0);
                b.kinds[i] = 1 + u8::from(g == PseudoElement::After);
            }
            if pseudo && generated.is_none() || !keep(i) {
                continue;
            }
            match (n.compound(comps, pseudo), n.lists(comps)) {
                (Key::Any, Some(keys)) => keys.into_iter().for_each(|k| b.place(k, i as u32)),
                (k, _) => b.place(k, i as u32),
            }
        }
        b
    }

    fn place(&mut self, key: Key, i: u32) {
        match key {
            Key::Id(a) => self.id.entry(a).or_default().push(i),
            Key::Class(a) => self.class.entry(a).or_default().push(i),
            Key::Local(a, l) | Key::Attr(a, l) => {
                let map = if matches!(key, Key::Local(..)) {
                    &mut self.local
                } else {
                    &mut self.attr
                };
                map.entry(a).or_default().push(i);
                if l != a {
                    map.entry(l).or_default().push(i);
                }
            }
            Key::Root => self.root.push(i),
            Key::Any => self.any.push(i),
            Key::Never => {}
        }
    }

    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
            && self.class.is_empty()
            && self.local.is_empty()
            && self.root.is_empty()
            && self.attr.is_empty()
            && self.any.is_empty()
    }

    /// Every rule of these buckets, each once.
    pub fn all(&self, out: &mut Vec<u32>) {
        out.clear();
        let maps = [&self.id, &self.class].into_iter().flat_map(|m| m.values());
        let names = [&self.local, &self.attr]
            .into_iter()
            .flat_map(|m| m.values());
        for v in maps.chain(names) {
            out.extend_from_slice(v);
        }
        out.extend_from_slice(&self.root);
        out.extend_from_slice(&self.any);
        out.sort_unstable();
        out.dedup();
    }

    /// The rules `el` may match, each once, in no particular order. Only
    /// the keyed ones are sorted to drop repeats (two classes or two
    /// attributes of one name, a rule under two names): those in none are
    /// most of them on a large stylesheet, and an unbucketed rule is in no bucket.
    pub fn candidates(&self, dom: &Dom, el: NodeId, out: &mut Vec<u32>) {
        out.clear();
        let mut add = |map: &HashMap<u64, Vec<u32>>, k: u64| {
            if let Some(v) = map.get(&k) {
                out.extend_from_slice(v);
            }
        };
        if self.quirks {
            if dom.element_id(el).is_some()
                && let Some(v) = dom.attribute(el, atom!("id"))
            {
                add(&self.id, caseless_hash(v.units()));
            }
            if !dom.class_list(el).1.is_empty()
                && let Some(v) = dom.attribute(el, atom!("class"))
            {
                for r in words(v) {
                    add(&self.class, caseless_hash(r.map(|i| v.unit(i))));
                }
            }
        } else {
            if let Some(id) = dom.element_id(el) {
                add(&self.id, u64::from(id.index()));
            }
            for c in dom.class_list(el).1 {
                add(&self.class, u64::from(c.index()));
            }
        }
        if let Some(n) = dom.local_name(el)
            && let Some(v) = self.local.get(&n.index())
        {
            out.extend_from_slice(v);
        }
        self.root_and_attributes(dom, el, out);
        out.sort_unstable();
        out.dedup();
        out.extend_from_slice(&self.any);
    }

    // the rules of `:root` when `el` is the root, and of each of its attributes' names
    fn root_and_attributes(&self, dom: &Dom, el: NodeId, out: &mut Vec<u32>) {
        if !self.root.is_empty()
            && dom
                .parent(el)
                .is_some_and(|p| dom.kind(p) == NodeKind::Document)
        {
            out.extend_from_slice(&self.root);
        }
        if !self.attr.is_empty() {
            for a in dom.attributes(el) {
                if let Some(v) = self.attr.get(&a.local.index()) {
                    out.extend_from_slice(v);
                }
            }
        }
    }
}

impl Run<'_, '_> {
    /// The rules that may match `el`, for `collect` and its signature.
    pub(crate) fn candidates(&mut self, el: NodeId) {
        let mut c = std::mem::take(&mut self.candidates);
        self.bucket(&self.prep.buckets, el, &mut c);
        self.candidates = c;
    }

    /// The rules of `b` that `el` may match.
    pub(crate) fn bucket(&self, b: &Buckets, el: NodeId, out: &mut Vec<u32>) {
        if self.every_rule {
            b.all(out);
        } else {
            b.candidates(self.m.dom, el, out);
        }
    }
}
