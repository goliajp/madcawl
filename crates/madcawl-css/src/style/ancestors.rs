//! What an element's ancestors have that selectors look for, hashed into a small filter: their ids, classes
//! and local names. A rule whose selector needs some ancestor to have an id, class or name the filter of
//! the element's ancestors lacks cannot match the element and is not tried. The filter may hold what no
//! ancestor has, never the other way round, so it only ever skips rules that would fail.

use std::cell::Cell;

use madcawl_dom::{Dom, NodeId};
use madcawl_str::atom;

use crate::selector::{Combinator, Component};

use super::caseless::{caseless_hash, words};
use super::compute::Run;
use super::rules::{Rules, Sel};
use super::styles::Styles;

/// A set of ids, classes and local names, two bits each of 256.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Filter([u64; 4]);

// keeps an id, a class and a local name with the same number apart
const ID: u64 = 0x5bd1_e995_9e37_79b9;
const CLASS: u64 = 0xc2b2_ae3d_27d4_eb4f;
const LOCAL: u64 = 0x1656_67b1_9e37_79f9;

impl Filter {
    fn add(&mut self, key: u64) {
        let mut h = key;
        h = (h ^ h >> 30).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h = (h ^ h >> 27).wrapping_mul(0x94d0_49bb_1331_11eb);
        h ^= h >> 31;
        for bit in [h & 255, h >> 8 & 255] {
            self.0[(bit >> 6) as usize] |= 1 << (bit & 63);
        }
    }

    fn holds(&self, o: &Filter) -> bool {
        (0..4).all(|k| self.0[k] & o.0[k] == o.0[k])
    }

    fn with(mut self, o: &Filter) -> Filter {
        for k in 0..4 {
            self.0[k] |= o.0[k];
        }
        self
    }
}

/// By rule, what its selector needs some ancestor of the element to have: the ids, classes and local names
/// of the compounds a child or descendant combinator leads to, as long as no sibling combinator follows
/// before the next one of those. Classes and ids are keyed as the buckets key them.
pub(crate) fn needs(rules: &Rules<'_>, quirks: bool) -> Vec<Filter> {
    let need = |e: &super::rules::Entry<'_>| {
        let mut f = Filter::default();
        let Sel::Complex(c) = e.sel else { return f };
        let data = &rules.sheets[e.sheet as usize];
        let atom = |n: crate::Name| u64::from(data.atoms[n.index()].index());
        let key = |n: crate::Name| {
            if quirks {
                caseless_hash(data.sheet.names().get(n).encode_utf16())
            } else {
                atom(n)
            }
        };
        let mut above = false;
        for comp in c.comps {
            match comp {
                Component::Combinator(k) => {
                    above = matches!(k, Combinator::Child | Combinator::Descendant);
                }
                Component::Id(n) if above => f.add(ID ^ key(*n)),
                Component::Class(n) if above => f.add(CLASS ^ key(*n)),
                // a type selector matches the name as written or lowercased, by the element: only a name
                // both are is needed
                Component::Type { name, lower, .. } if above && atom(*name) == atom(*lower) => {
                    f.add(LOCAL ^ atom(*lower));
                }
                _ => {}
            }
        }
        f
    };
    rules.entries.iter().map(need).collect()
}

// `el`'s own id, classes and local name
fn own(dom: &Dom, el: NodeId, quirks: bool) -> Filter {
    let mut f = Filter::default();
    if quirks {
        if dom.element_id(el).is_some()
            && let Some(v) = dom.attribute(el, atom!("id"))
        {
            f.add(ID ^ caseless_hash(v.units()));
        }
        if !dom.class_list(el).1.is_empty()
            && let Some(v) = dom.attribute(el, atom!("class"))
        {
            for r in words(v) {
                f.add(CLASS ^ caseless_hash(r.map(|i| v.unit(i))));
            }
        }
    } else {
        if let Some(id) = dom.element_id(el) {
            f.add(ID ^ u64::from(id.index()));
        }
        for c in dom.class_list(el).1 {
            f.add(CLASS ^ u64::from(c.index()));
        }
    }
    if let Some(n) = dom.local_name(el) {
        f.add(LOCAL ^ u64::from(n.index()));
    }
    f
}

/// The filters from the root down to the element last styled, each of an element and its ancestors, and
/// the element being styled with the filter of its ancestors.
#[derive(Default)]
pub(crate) struct Ancestors {
    chain: Vec<(NodeId, Filter)>,
    above: Option<(NodeId, Filter)>,
    /// Every rule is tried
    pub off: bool,
    /// Rules asked about and ruled out since the counts were last taken
    checked: Cell<u64>,
    skipped: Cell<u64>,
}

impl Run<'_, '_> {
    /// Makes the filter of `el`'s ancestors the current one. The chain of filters from the root follows the
    /// elements styled, each after its parent; an element whose parent is not on it starts it again from
    /// the root.
    pub(crate) fn enter(&mut self, el: NodeId) {
        let (m, quirks, a) = (&self.m, self.prep.quirks, &mut self.ancestors);
        let parent = m.parent_element(el);
        while a.chain.last().is_some_and(|&(n, _)| Some(n) != parent) {
            a.chain.pop();
        }
        if a.chain.is_empty() {
            let mut up = parent;
            while let Some(n) = up {
                a.chain.push((n, own(m.dom, n, quirks)));
                up = m.parent_element(n);
            }
            a.chain.reverse();
            for k in 1..a.chain.len() {
                a.chain[k].1 = a.chain[k].1.with(&a.chain[k - 1].1);
            }
        }
        let above = a.chain.last().map_or(Filter::default(), |&(_, f)| f);
        a.above = Some((el, above));
        a.chain.push((el, own(m.dom, el, quirks).with(&above)));
    }

    /// Whether rule `c` may match `el`: false only when it needs an ancestor to have what none has.
    pub(crate) fn may_match(&self, c: u32, el: NodeId) -> bool {
        let a = &self.ancestors;
        let Some((_, f)) = a.above.filter(|&(n, _)| n == el && !a.off) else {
            return true;
        };
        a.checked.set(a.checked.get() + 1);
        let may = f.holds(&self.prep.needs[c as usize]);
        a.skipped.set(a.skipped.get() + u64::from(!may));
        may
    }

    /// Adds the rules checked and skipped since the last call to `out`'s counts.
    pub(crate) fn count_filtered(&self, out: &mut Styles) {
        out.stats.filter_checked += self.ancestors.checked.take();
        out.stats.filter_skipped += self.ancestors.skipped.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_holds_what_was_added_to_it_and_what_it_was_joined_with() {
        let (mut a, mut b) = (Filter::default(), Filter::default());
        a.add(ID ^ 7);
        b.add(CLASS ^ 7);
        let ab = a.with(&b);
        assert!(ab.holds(&a) && ab.holds(&b) && !a.holds(&ab));
        assert!(Filter::default().holds(&Filter::default()));
    }
}
