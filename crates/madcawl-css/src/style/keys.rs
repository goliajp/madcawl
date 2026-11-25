//! Which bucket a rule goes in: the key of the rightmost compound of its
//! selector, by the names of its sheet.

use crate::selector::{Component, PseudoClass, PseudoElement};

use super::caseless::caseless_hash;
use super::rules::SheetData;

pub(super) enum Key {
    Id(u64),
    Class(u64),
    Local(u32, u32),
    Root,
    Attr(u32, u32),
    Any,
    /// A pseudo-element: it matches no element
    Never,
}

/// The names of one sheet as the buckets key them.
pub(super) struct Namer<'r> {
    pub data: &'r SheetData<'r>,
    pub quirks: bool,
}

impl Namer<'_> {
    fn atom(&self, n: crate::Name) -> u32 {
        self.data.atoms[n.index()].index()
    }

    fn key(&self, n: crate::Name) -> u64 {
        if self.quirks {
            caseless_hash(self.data.sheet.names().get(n).encode_utf16())
        } else {
            u64::from(self.atom(n))
        }
    }

    /// The key of the compound `comps` starts with (up to its first combinator): its id, else a class,
    /// else its name, else `:root`, else an attribute's name. A pseudo-element other than `::before` and
    /// `::after` for `pseudo`, which matches no element, gives `Never`.
    pub fn compound(&self, comps: &[Component], pseudo: bool) -> Key {
        let (mut key, mut fallback, mut id) = (Key::Any, Key::Any, None);
        for comp in comps {
            key = match comp {
                Component::Combinator(_) => break,
                Component::PseudoElement(PseudoElement::Before | PseudoElement::After)
                    if pseudo =>
                {
                    continue;
                }
                Component::PseudoElement(_) => return Key::Never,
                Component::Id(n) => {
                    id.get_or_insert(self.key(*n));
                    continue;
                }
                Component::Class(n) => Key::Class(self.key(*n)),
                Component::Type { name, lower, .. } if matches!(key, Key::Any) => {
                    Key::Local(self.atom(*name), self.atom(*lower))
                }
                Component::PseudoClass(PseudoClass::Root) => {
                    fallback = Key::Root;
                    continue;
                }
                Component::Attribute(a) if !matches!(fallback, Key::Root) => {
                    fallback = Key::Attr(self.atom(a.name), self.atom(a.lower));
                    continue;
                }
                _ => continue,
            };
        }
        match (id, key) {
            (Some(i), _) => Key::Id(i),
            (None, Key::Any) => fallback,
            (None, k) => k,
        }
    }

    /// With no key of its own, the keys of an `:is()` or `:where()` of the compound whose every selector
    /// has one: an element it matches matches one of them. Those that match no element are left out.
    pub fn lists(&self, comps: &[Component]) -> Option<Vec<Key>> {
        let lists = comps
            .iter()
            .take_while(|c| !matches!(c, Component::Combinator(_)));
        lists
            .filter_map(|c| match c {
                Component::Is(l) | Component::Where(l) => Some(l),
                _ => None,
            })
            .find_map(|l| {
                let keys: Vec<Key> = l
                    .iter()
                    .map(|s| self.compound(&s.components, false))
                    .collect();
                let keyed = keys.iter().all(|k| !matches!(k, Key::Any));
                keyed.then(|| {
                    keys.into_iter()
                        .filter(|k| !matches!(k, Key::Never))
                        .collect()
                })
            })
    }
}
