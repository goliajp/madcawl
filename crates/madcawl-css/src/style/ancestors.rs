//! What an element's ancestors have that selectors look for, hashed into a filter: their ids, classes and
//! local names. A rule whose selector needs some ancestor to have an id, class or name the filter of the
//! element's ancestors lacks cannot match the element and is not tried. The filter may hold what no
//! ancestor has, never the other way round, so it only ever skips rules that would fail.
//!
//! A rule the filter lets through by mistake can cost far more than one match: a selector with several
//! descendant combinators is tried against every ancestor in turn. Pages of utility classes put a few hundred
//! names on an element's ancestors, so the filter is large and each name sets three of its bits; with a
//! small one, which rules got through would depend on the names' atom numbers, that is on what the process
//! loaded before. It counts, by bit, the ancestors that set it: entering an element adds its parent's names
//! and leaving one takes them back, so no filter is copied per element.

use std::cell::Cell;

use madcawl_dom::{Dom, NodeId};
use madcawl_str::atom;

use crate::selector::{Combinator, Component};

use super::caseless::{caseless_hash, words};
use super::compute::Run;
use super::rules::{Rules, Sel};
use super::styles::Styles;

const BITS: usize = 4096;
/// Bits a name sets
const K: usize = 3;
const SHIFT: u32 = BITS.trailing_zeros();

/// What a rule needs of an element's ancestors: the filter bits of up to eight different names. Names past
/// those are not checked, which only lets more rules through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Need {
    bits: [u16; 8 * K],
    len: u8,
}

impl Need {
    fn add(&mut self, key: u64) {
        let n = usize::from(self.len);
        let new = bits(key);
        // a name met again (`div div span`) takes no room from the ones further left
        if n < self.bits.len() && !self.bits[..n].chunks(K).any(|b| b == new) {
            self.bits[n..n + K].copy_from_slice(&new);
            self.len += K as u8;
        }
    }
}

// the bits of a key
fn bits(key: u64) -> [u16; K] {
    let mut h = key;
    h = (h ^ h >> 30).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h = (h ^ h >> 27).wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^= h >> 31;
    std::array::from_fn(|i| (h >> (SHIFT * i as u32)) as u16 & (BITS - 1) as u16)
}

// keeps an id, a class and a local name with the same number apart
const ID: u64 = 0x5bd1_e995_9e37_79b9;
const CLASS: u64 = 0xc2b2_ae3d_27d4_eb4f;
const LOCAL: u64 = 0x1656_67b1_9e37_79f9;

/// By rule, what its selector needs some ancestor of the element to have: the ids, classes and local names
/// of the compounds a child or descendant combinator leads to, as long as no sibling combinator follows
/// before the next one of those. Classes and ids are keyed as the buckets key them.
pub(crate) fn needs(rules: &Rules<'_>, quirks: bool) -> Vec<Need> {
    let need = |e: &super::rules::Entry<'_>| {
        let mut f = Need::default();
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

// the bits of `el`'s own id, classes and local name onto `out`
fn own(out: &mut Vec<u16>, dom: &Dom, el: NodeId, quirks: bool) {
    let mut add = |key: u64| out.extend_from_slice(&bits(key));
    if quirks {
        if dom.element_id(el).is_some()
            && let Some(v) = dom.attribute(el, atom!("id"))
        {
            add(ID ^ caseless_hash(v.units()));
        }
        if !dom.class_list(el).1.is_empty()
            && let Some(v) = dom.attribute(el, atom!("class"))
        {
            for r in words(v) {
                add(CLASS ^ caseless_hash(r.map(|i| v.unit(i))));
            }
        }
    } else {
        if let Some(id) = dom.element_id(el) {
            add(ID ^ u64::from(id.index()));
        }
        for c in dom.class_list(el).1 {
            add(CLASS ^ u64::from(c.index()));
        }
    }
    if let Some(n) = dom.local_name(el) {
        add(LOCAL ^ u64::from(n.index()));
    }
}

/// The elements from the root down to the one being styled, and by bit how many of those but the last set
/// it: the filter of the last one's ancestors.
#[derive(Default)]
pub(crate) struct Ancestors {
    /// Each element, where its bits start in `bits`, and whether they are counted
    chain: Vec<(NodeId, u32, bool)>,
    bits: Vec<u16>,
    /// A count stuck at the largest value is never taken back: it only keeps the bit set
    counts: Vec<u16>,
    at: Option<NodeId>,
    /// Every rule is tried
    pub off: bool,
    /// Rules asked about and ruled out since the counts were last taken
    checked: Cell<u64>,
    skipped: Cell<u64>,
}

impl Ancestors {
    fn push(&mut self, el: NodeId, dom: &Dom, quirks: bool) {
        let start = self.bits.len() as u32;
        own(&mut self.bits, dom, el, quirks);
        self.chain.push((el, start, false));
    }

    // counts the bits of the last element on the chain, its children being entered
    fn count_last(&mut self) {
        let Some(last) = self.chain.last_mut().filter(|l| !l.2) else {
            return;
        };
        last.2 = true;
        if self.counts.is_empty() {
            self.counts = vec![0; BITS];
        }
        for &b in &self.bits[last.1 as usize..] {
            let c = &mut self.counts[usize::from(b)];
            *c = c.saturating_add(1);
        }
    }

    fn pop(&mut self) {
        let Some((_, start, counted)) = self.chain.pop() else {
            return;
        };
        if counted {
            for &b in &self.bits[start as usize..] {
                let c = &mut self.counts[usize::from(b)];
                if *c != u16::MAX {
                    *c -= 1;
                }
            }
        }
        self.bits.truncate(start as usize);
    }

    fn holds(&self, n: &Need) -> bool {
        n.bits[..usize::from(n.len)]
            .iter()
            .all(|&b| self.counts.get(usize::from(b)).is_some_and(|&c| c != 0))
    }
}

impl Run<'_, '_> {
    /// Makes the filter of `el`'s ancestors the current one. The chain from the root follows the elements
    /// styled, each after its parent; an element whose parent is not on it starts it again from the root.
    pub(crate) fn enter(&mut self, el: NodeId) {
        let (m, quirks, a) = (&self.m, self.prep.quirks, &mut self.ancestors);
        let parent = m.parent_element(el);
        while a.chain.last().is_some_and(|&(n, _, _)| Some(n) != parent) {
            a.pop();
        }
        if a.chain.is_empty()
            && let Some(p) = parent
        {
            let mut up = vec![p];
            while let Some(n) = m.parent_element(*up.last().expect("one at least")) {
                up.push(n);
            }
            for &n in up.iter().rev() {
                a.count_last();
                a.push(n, m.dom, quirks);
            }
        }
        a.count_last();
        a.push(el, m.dom, quirks);
        a.at = Some(el);
    }

    /// Whether rule `c` may match `el`: false only when it needs an ancestor to have what none has.
    pub(crate) fn may_match(&self, c: u32, el: NodeId) -> bool {
        let a = &self.ancestors;
        if a.off || a.at != Some(el) {
            return true;
        }
        a.checked.set(a.checked.get() + 1);
        let may = a.holds(&self.prep.needs[c as usize]);
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

    // an ancestors filter that counted the names `keys`
    fn counted(keys: &[u64]) -> Ancestors {
        let mut a = Ancestors::default();
        for (i, &k) in keys.iter().enumerate() {
            let start = a.bits.len() as u32;
            a.bits.extend_from_slice(&bits(k));
            a.chain.push((
                NodeId::from_index(i as u32 + 1).expect("not 0"),
                start,
                false,
            ));
            a.count_last();
        }
        a
    }

    fn need(keys: &[u64]) -> Need {
        let mut n = Need::default();
        keys.iter().for_each(|&k| n.add(k));
        n
    }

    #[test]
    fn the_filter_holds_what_its_ancestors_have_until_they_leave() {
        let mut a = counted(&[ID ^ 7, CLASS ^ 7, CLASS ^ 7]);
        assert!(a.holds(&need(&[ID ^ 7, CLASS ^ 7])) && !a.holds(&need(&[CLASS ^ 8])));
        assert!(a.holds(&Need::default()));
        a.pop();
        assert!(a.holds(&need(&[CLASS ^ 7])), "another ancestor has it");
        a.pop();
        assert!(!a.holds(&need(&[CLASS ^ 7])) && a.holds(&need(&[ID ^ 7])));
        a.pop();
        assert!(a.counts.iter().all(|&c| c == 0));
    }

    #[test]
    fn a_need_keeps_eight_different_names_and_a_name_once() {
        let mut keys = Vec::new();
        for k in 0..8 {
            keys.extend([LOCAL ^ 1, CLASS ^ k]);
        }
        let n = need(&keys);
        // the local name, met eight times, took one place: the eighth class is past the end
        let mut have = vec![LOCAL ^ 1];
        for k in 0..7 {
            assert!(!counted(&have).holds(&n));
            have.push(CLASS ^ k);
        }
        assert!(counted(&have).holds(&n));
    }
}
