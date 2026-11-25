//! `PathKey`: a number for the chain of what selectors can see of each
//! element from the root down to one element. Each element's own part
//! (`own_sig`) is its name and namespace, its id and classes if some
//! selector names them (by a number for each distinct text), which of the attributes selectors name it has,
//! its style attribute and presentational hints (by a number for each
//! distinct text, not a hash, so that no two share), and which of the rules
//! that need more than the element and its ancestors match it: those
//! with sibling combinators, pseudo-classes, `:has()` or attribute
//! values (`Features::reval`). Two elements with the same
//! key match the same rules and have the same parent values, so they
//! get the same computed values: the second one is two table lookups.
//!
//! The tables live with the document's styles; they hold at most one
//! key per element styled since the last full run, and a full run with
//! a new set of stylesheets starts them empty.

use std::collections::HashMap;

use madcawl_dom::NodeId;
use madcawl_str::atom;

use super::Styles;
use super::before_after::PseudoMemo;
use super::compute::Run;
use super::features::name_key;
use super::hints::hints;
use super::matching::text;

/// The parent key of the root element.
pub(crate) const ROOT: u32 = u32::MAX;

const STYLE: u64 = 1 << 63;
const HINT: u64 = 1 << 62;
const REVAL: u64 = 1 << 61;

#[derive(Default)]
pub(crate) struct PathKeys {
    sigs: HashMap<Box<[u64]>, u32>,
    /// Style attributes, presentational hints, ids and class names by
    /// their text: a signature holds the number, so two texts never
    /// share one
    texts: HashMap<Box<[u8]>, u64>,
    /// The bytes the keys of `sigs` and `texts` hold
    held: usize,
    keys: HashMap<(u32, u32), u32>,
    /// By key, once an element with the key has been styled in full:
    /// the style's place in the table, the custom property set, and what
    /// it leaves about its pseudo-elements
    cache: Vec<Option<(u32, u32, PseudoMemo)>>,
}

impl PathKeys {
    fn sig(&mut self, parts: &[u64]) -> u32 {
        if let Some(&s) = self.sigs.get(parts) {
            return s;
        }
        let s = self.sigs.len() as u32;
        self.held += size_of_val(parts);
        self.sigs.insert(parts.into(), s);
        s
    }

    fn key(&mut self, parent: u32, sig: u32) -> u32 {
        if let Some(&k) = self.keys.get(&(parent, sig)) {
            return k;
        }
        let k = self.cache.len() as u32;
        self.keys.insert((parent, sig), k);
        self.cache.push(None);
        k
    }

    fn text(&mut self, t: &str) -> u64 {
        self.bytes(t.as_bytes())
    }

    /// The number of a text, WTF-8.
    fn bytes(&mut self, t: &[u8]) -> u64 {
        if let Some(&n) = self.texts.get(t) {
            return n;
        }
        let n = self.texts.len() as u64;
        self.held += t.len();
        self.texts.insert(t.into(), n);
        n
    }

    /// The number of an atom's text.
    fn name(&mut self, a: madcawl_str::Atom) -> u64 {
        let mut v = Vec::new();
        a.to_dom_string().write_wtf8(&mut v);
        self.bytes(&v)
    }

    /// Keys; the number of texts grows no faster, as each new text
    /// makes a new signature and so a new key, but their bytes do: see
    /// `held`
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// The bytes the signatures and texts hold.
    pub fn held(&self) -> usize {
        self.held
    }
}

impl Run<'_, '_> {
    /// `el`'s own signature; `candidates` holds its rules.
    fn own_sig(&mut self, el: NodeId, pk: &mut PathKeys) -> u32 {
        let dom = self.m.dom;
        let f = &self.prep.features;
        self.sig.clear();
        let name = dom.local_name(el).map_or(0, |a| u64::from(a.index()));
        let ns = dom.namespace(el).map_or(9, |n| n as u64);
        self.sig.push(name | ns << 32);
        // only the id and classes some selector names, by their text: the
        // DOM gives back id atoms and class list numbers no element holds
        // any more, and may give them to other values later
        let id = dom
            .element_id(el)
            .filter(|a| f.id.contains_key(&name_key(&text(&a.to_dom_string()))));
        self.sig.push(id.map_or(u64::MAX, |a| pk.name(a)));
        // a list number names one list while the DOM is borrowed, for
        // the whole run
        let (list, classes) = dom.class_list(el);
        let named = self.named.entry(list).or_insert_with(|| {
            classes
                .iter()
                .filter(|c| f.class.contains_key(&name_key(&text(&c.to_dom_string()))))
                .map(|&c| pk.name(c))
                .collect()
        });
        self.sig.push(named.len() as u64);
        self.sig.extend_from_slice(named);
        for a in dom.attributes(el) {
            if f.attrs.contains(&a.local.index()) {
                self.sig
                    .push(u64::from(a.local.index()) | (a.ns as u64) << 32);
            }
        }
        if let Some(s) = dom.attribute(el, atom!("style")) {
            self.sig.push(STYLE);
            self.sig.push(pk.text(&text(s)));
        }
        if let Some(h) = hints(dom, el) {
            self.sig.push(HINT);
            self.sig.push(pk.text(&h));
        }
        self.sig.push(REVAL);
        let rules = self.m.rules;
        for &c in &self.candidates {
            if f.reval[c as usize]
                && self.may_match(c, el)
                && self.m.entry(&rules.entries[c as usize], el)
            {
                self.sig.push(u64::from(c));
            }
        }
        pk.sig(&self.sig)
    }

    /// Style `el`, its parent already styled: by its `PathKey` when an
    /// element with the same key was styled before, in full otherwise.
    /// Whether its values, custom properties or flags changed, and whether its `::before` or `::after`
    /// values did.
    pub(crate) fn style_one(&mut self, el: NodeId, out: &mut Styles) -> (bool, bool) {
        let i = el.index() as usize;
        out.grow(i);
        self.enter(el);
        let parent = self.m.parent_element(el);
        self.candidates(el);
        let before = (out.of[i], out.flags[i], out.customs[i]);
        let pseudo_before = out.pseudos.get(&el.index()).copied();
        let pkey = parent.map_or(ROOT, |p| out.keys[p.index() as usize]);
        let sig = self.own_sig(el, &mut out.pk);
        let key = out.pk.key(pkey, sig);
        out.keys[i] = key;
        out.stats.elements += 1;
        let hit = if self.path_key {
            out.pk.cache[key as usize]
        } else {
            None
        };
        if let Some((k, set, _)) = hit {
            // a hit substitutes nothing, so it spends none of the
            // `var()` budget: the budget bounds substitution work, and an
            // element with the key paid for it when it was styled in full
            out.stats.hits += 1;
            if self.verify {
                self.verify_hit(el, parent, (k, set), out);
            }
            out.of[i] = k + 1;
            out.customs[i] = set;
        } else {
            self.collect(el);
            let s = self.element(el, parent, out);
            let k = out.intern(s);
            out.of[i] = k + 1;
        }
        let s = &out.table[out.of[i] as usize - 1];
        if self.m.is_root(el) {
            out.root_font = s.font.size();
        }
        let display = s.display;
        let above = parent.map_or(0, |p| {
            self.m
                .flags_below(p, out.get(p), out.flags[p.index() as usize], el)
        });
        out.flags[i] = above | self.m.own_flags(el, display);
        match hit {
            Some((_, _, memo)) => self.pseudos_by_key(el, memo, out),
            None => {
                let memo = self.pseudos_memo(el, out);
                if self.path_key {
                    out.pk.cache[key as usize] = Some((out.of[i] - 1, out.customs[i], memo));
                }
            }
        }
        self.count_filtered(out);
        (
            before != (out.of[i], out.flags[i], out.customs[i]),
            pseudo_before != out.pseudos.get(&el.index()).copied(),
        )
    }

    /// Style `el` in full and compare with what its key gave; the budget
    /// of `var()` substitution is left as it was.
    fn verify_hit(
        &mut self,
        el: NodeId,
        parent: Option<NodeId>,
        got: (u32, u32),
        out: &mut Styles,
    ) {
        let budget = out.memo.budget.get();
        self.collect(el);
        let s = self.element(el, parent, out);
        out.memo.budget.set(budget);
        out.stats.verified += 1;
        let set = out.customs[el.index() as usize];
        if s != out.table[got.0 as usize] || set != got.1 {
            out.stats.mismatches += 1;
            if out.stats.first_mismatch.is_none() {
                out.stats.first_mismatch = Some(el);
            }
        }
    }
}
