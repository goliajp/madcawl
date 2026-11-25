//! Rules by the most particular simple selector of their rightmost
//! compound: an id, a class, a local name, or none. An element is
//! tried against the buckets of its id, its classes and its name, and
//! the rules in none.

use std::collections::HashMap;

use madcawl_dom::{Dom, NodeId};
use madcawl_str::atom;

use crate::selector::{Component, PseudoElement};

use super::caseless::{caseless_hash, words};
use super::decls::What;
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
    any: Vec<u32>,
    /// By rule, for the pseudo-element buckets, 1 for a `::before` rule and 2 for an `::after` one
    pub kinds: Vec<u8>,
}

enum Key {
    Id(u64),
    Class(u64),
    Local(u32, u32),
    Any,
    /// A pseudo-element: it matches no element
    Never,
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
    pub fn build(rules: &Rules<'_>, quirks: bool, pseudo: bool, keep: impl Fn(usize) -> bool) -> Buckets {
        let mut b = Buckets {
            quirks,
            ..Buckets::default()
        };
        for (i, e) in rules.entries.iter().enumerate() {
            let data = &rules.sheets[e.sheet as usize];
            let atom = |n: crate::Name| data.atoms[n.index()].index();
            let key_of = |n: crate::Name| {
                if quirks {
                    caseless_hash(data.sheet.names().get(n).encode_utf16())
                } else {
                    u64::from(atom(n))
                }
            };
            let mut key = Key::Any;
            let mut generated = false;
            if let Sel::Complex(c) = e.sel {
                for comp in c.comps {
                    let k = match comp {
                        Component::Combinator(_) => break,
                        Component::PseudoElement(
                            e @ (PseudoElement::Before | PseudoElement::After),
                        ) if pseudo => {
                            generated = true;
                            b.kinds.resize(rules.entries.len(), 0);
                            b.kinds[i] = 1 + u8::from(*e == PseudoElement::After);
                            continue;
                        }
                        Component::PseudoElement(_) => Key::Never,
                        Component::Id(n) => Key::Id(key_of(*n)),
                        Component::Class(n) if !matches!(key, Key::Id(_)) => Key::Class(key_of(*n)),
                        Component::Type { name, lower, .. } if matches!(key, Key::Any) => {
                            Key::Local(atom(*name), atom(*lower))
                        }
                        _ => continue,
                    };
                    if matches!(key, Key::Never) {
                        break;
                    }
                    key = k;
                }
            }
            let i = i as u32;
            if pseudo && !generated || !keep(i as usize) {
                key = Key::Never;
            }
            match key {
                Key::Id(a) => b.id.entry(a).or_default().push(i),
                Key::Class(a) => b.class.entry(a).or_default().push(i),
                Key::Local(a, l) => {
                    b.local.entry(a).or_default().push(i);
                    if l != a {
                        b.local.entry(l).or_default().push(i);
                    }
                }
                Key::Any => b.any.push(i),
                Key::Never => {}
            }
        }
        b
    }

    pub fn is_empty(&self) -> bool {
        self.id.is_empty() && self.class.is_empty() && self.local.is_empty() && self.any.is_empty()
    }

    /// The rules `el` may match, each once, in no particular order.
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
        out.extend_from_slice(&self.any);
        out.sort_unstable();
        out.dedup();
    }
}
