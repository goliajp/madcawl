//! Rules by the most particular simple selector of their rightmost
//! compound: an id, a class, a local name, or none. An element is
//! tried against the buckets of its id, its classes and its name, and
//! the rules in none.

use std::collections::HashMap;

use madcawl_dom::{Dom, NodeId};

use crate::selector::Component;

use super::rules::{Rules, Sel};

#[derive(Default)]
pub(crate) struct Buckets {
    id: HashMap<u32, Vec<u32>>,
    class: HashMap<u32, Vec<u32>>,
    local: HashMap<u32, Vec<u32>>,
    any: Vec<u32>,
}

enum Key {
    Id(u32),
    Class(u32),
    Local(u32, u32),
    Any,
    /// A pseudo-element: it matches no element
    Never,
}

impl Buckets {
    /// In quirks mode ids and classes match ASCII case-insensitively,
    /// which atoms cannot look up, so they are not keys.
    pub fn build(rules: &Rules<'_>, quirks: bool) -> Buckets {
        let mut b = Buckets::default();
        for (i, e) in rules.entries.iter().enumerate() {
            let atoms = &rules.sheets[e.sheet as usize].atoms;
            let atom = |n: crate::Name| atoms[n.index()].index();
            let mut key = Key::Any;
            if let Sel::Complex(c) = e.sel {
                for comp in c.comps {
                    let k = match comp {
                        Component::Combinator(_) => break,
                        Component::PseudoElement(_) => Key::Never,
                        Component::Id(n) if !quirks => Key::Id(atom(*n)),
                        Component::Class(n) if !quirks && !matches!(key, Key::Id(_)) => {
                            Key::Class(atom(*n))
                        }
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

    /// The rules `el` may match, each once, in no particular order.
    pub fn candidates(&self, dom: &Dom, el: NodeId, out: &mut Vec<u32>) {
        out.clear();
        if let Some(id) = dom.element_id(el)
            && let Some(v) = self.id.get(&id.index())
        {
            out.extend_from_slice(v);
        }
        for c in dom.class_list(el).1 {
            if let Some(v) = self.class.get(&c.index()) {
                out.extend_from_slice(v);
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
