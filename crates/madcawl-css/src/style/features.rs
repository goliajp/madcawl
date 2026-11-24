//! What the rules' selectors look at, collected once per set of
//! stylesheets: the attribute names any selector tests (an element's
//! `PathKey` signature includes those attributes), the rules whose match
//! depends on more than the element and its ancestors (revalidated for
//! each element), and for each class, id, attribute and pseudo-class,
//! which elements a change to it can reach (`Tier`).

use std::collections::{HashMap, HashSet};

use crate::selector::{Combinator, Component, NthKind, PseudoClass, SelectorList};

use super::caseless::caseless_hash;
use super::rules::{NONE, Rules, Sel};

/// Which elements a change to an element's feature may restyle: a set
/// of ranges, one for each kind of place the feature has in selectors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Tier(u8);

impl Tier {
    pub const NONE: Tier = Tier(0);
    /// In a rightmost compound: the element
    pub const OWN: Tier = Tier(1);
    /// Left of a sibling combinator: the siblings after the element
    pub const SIBLINGS: Tier = Tier(2);
    /// Left of a sibling combinator that is left of a descendant or
    /// child one: those siblings and their subtrees
    pub const SIBLING_TREES: Tier = Tier(4);
    /// Left of a descendant or child combinator: the element's subtree
    pub const SUBTREE: Tier = Tier(8);
    /// In the `of S` of `:nth-child()` or `:nth-last-child()`, where it
    /// moves the siblings before and after: the parent's subtree
    pub const PARENT_SUBTREE: Tier = Tier(16);
    /// In `:has()`, which ancestors and earlier siblings can depend on:
    /// the document
    pub const FULL: Tier = Tier(32);

    pub fn or(self, o: Tier) -> Tier {
        Tier(self.0 | o.0)
    }

    pub fn has(self, o: Tier) -> bool {
        self.0 & o.0 != 0
    }

    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// A feature's key: classes and ids by their ASCII-lowercase text,
/// which covers quirks mode's case-insensitive matching.
pub(crate) fn name_key(s: &str) -> u64 {
    caseless_hash(s.encode_utf16())
}

#[derive(Default)]
pub(crate) struct Features {
    /// The atoms (as written and lowercase) of the attribute names
    /// selectors test; a rule that tests a value is revalidated
    pub attrs: HashSet<u32>,
    /// By entry: its match depends on siblings, descendants or state
    pub reval: Vec<bool>,
    pub class: HashMap<u64, Tier>,
    pub id: HashMap<u64, Tier>,
    /// By the lowercase name's key
    pub attr: HashMap<u64, Tier>,
    pub pseudo: HashMap<PseudoClass, Tier>,
    /// `:lang()` and `:dir()`, which read ancestors' attributes
    pub lang_dir: Tier,
    /// What appending an element changes: `:last-child` and
    /// `:only-child` on the element before it, their `of type` forms on
    /// the one before it of its type, `:nth-last-*()` on all before it
    pub last_child: Tier,
    pub last_of_type: Tier,
    pub nth_last: Tier,
    pub empty: Tier,
    pub has: bool,
}

/// Where a walk is in a complex selector: the outermost combinator
/// crossed so far, and whether a descendant or child one was crossed.
#[derive(Clone, Copy, Default)]
struct At {
    last: Option<Combinator>,
    descended: bool,
    /// The tier of the compound containing a nested list
    floor: Tier,
}

impl At {
    fn tier(self) -> Tier {
        let t = match self.last {
            None => Tier::OWN,
            Some(Combinator::Descendant | Combinator::Child) => Tier::SUBTREE,
            Some(_) if self.descended => Tier::SIBLING_TREES,
            Some(_) => Tier::SIBLINGS,
        };
        t.or(self.floor)
    }

    fn cross(self, c: Combinator) -> At {
        At {
            last: Some(c),
            descended: self.descended || matches!(c, Combinator::Descendant | Combinator::Child),
            floor: self.floor,
        }
    }
}

fn raise<K: std::hash::Hash + Eq>(m: &mut HashMap<K, Tier>, k: K, t: Tier) {
    let e = m.entry(k).or_default();
    *e = e.or(t);
}

struct Walk<'r, 'a> {
    rules: &'r Rules<'a>,
    f: Features,
    /// Whether the entry walked so far depends on more than the element
    /// and its ancestors
    reval: bool,
    sheet: u32,
}

impl Walk<'_, '_> {
    fn name(&self, n: crate::Name) -> &str {
        self.rules.sheets[self.sheet as usize].sheet.names().get(n)
    }

    fn atom(&self, n: crate::Name) -> u32 {
        self.rules.sheets[self.sheet as usize].atoms[n.index()].index()
    }

    fn complex(&mut self, comps: &[Component], mut at: At) -> At {
        for c in comps {
            match c {
                Component::Combinator(k) => {
                    if matches!(k, Combinator::NextSibling | Combinator::SubsequentSibling) {
                        self.reval = true;
                    }
                    at = at.cross(*k);
                }
                c => self.simple(c, at),
            }
        }
        at
    }

    fn list(&mut self, l: &SelectorList, at: At) {
        let floor = at.tier();
        // a sibling combinator in a list left of a descendant or child
        // combinator reaches the siblings' subtrees
        let inner = At {
            floor,
            descended: floor.has(Tier::SUBTREE.or(Tier::SIBLING_TREES)),
            ..At::default()
        };
        for s in l.iter() {
            self.complex(&s.components, inner);
        }
    }

    fn simple(&mut self, c: &Component, at: At) {
        let t = at.tier();
        match c {
            Component::Id(n) => {
                let k = name_key(self.name(*n));
                raise(&mut self.f.id, k, t)
            }
            Component::Class(n) => {
                let k = name_key(self.name(*n));
                raise(&mut self.f.class, k, t)
            }
            Component::Attribute(a) => {
                // a value is tested per element; only names go into keys
                self.reval |= a.op.is_some();
                self.f.attrs.insert(self.atom(a.name));
                self.f.attrs.insert(self.atom(a.lower));
                let lower = self.name(a.lower).to_owned();
                raise(&mut self.f.attr, name_key(&lower), t);
            }
            Component::PseudoClass(p) => {
                self.reval = true;
                raise(&mut self.f.pseudo, *p, t);
                use PseudoClass as P;
                match p {
                    P::LastChild | P::OnlyChild => self.f.last_child = self.f.last_child.or(t),
                    P::LastOfType | P::OnlyOfType => {
                        self.f.last_of_type = self.f.last_of_type.or(t)
                    }
                    P::Empty => self.f.empty = self.f.empty.or(t),
                    _ => {}
                }
            }
            Component::Nth(n) => {
                self.reval = true;
                if matches!(n.kind, NthKind::LastChild | NthKind::LastOfType) {
                    self.f.nth_last = self.f.nth_last.or(t);
                }
                if let Some(of) = &n.of {
                    let at = At {
                        floor: at.floor.or(Tier::PARENT_SUBTREE),
                        ..at
                    };
                    self.list(of, at);
                }
            }
            Component::Lang(_) | Component::Dir(_) => {
                self.reval = true;
                self.f.lang_dir = self.f.lang_dir.or(t);
            }
            Component::Not(l) | Component::Is(l) | Component::Where(l) => self.list(l, at),
            Component::Has(l) => {
                self.reval = true;
                self.f.has = true;
                let full = At {
                    floor: Tier::FULL,
                    ..At::default()
                };
                self.list(l, full);
            }
            Component::Nesting | Component::Universal(_) | Component::Type { .. } => {}
            Component::PseudoElement(_) | Component::Combinator(_) => {}
        }
    }

    /// The selectors of the rule `parent`, and of the rules it is
    /// nested in, as compounds further left of what was walked, `at`.
    fn parents(&mut self, parent: u32, at: At) {
        if parent == NONE {
            return;
        }
        let p = &self.rules.parents[parent as usize];
        let (mut far, mut nested) = (at, false);
        for c in &p.sels {
            let end = self.complex(c.comps, at);
            if c.relative || has_nesting(c.comps) {
                nested = true;
                far.floor = far.floor.or(end.tier());
            }
        }
        if nested {
            self.parents(p.parent, far);
        }
    }
}

fn has_nesting(comps: &[Component]) -> bool {
    comps.iter().any(|x| matches!(x, Component::Nesting))
}

impl Features {
    pub fn build(rules: &Rules<'_>) -> Features {
        let mut w = Walk {
            rules,
            f: Features::default(),
            reval: false,
            sheet: 0,
        };
        w.f.reval = Vec::with_capacity(rules.entries.len());
        for e in &rules.entries {
            w.reval = false;
            w.sheet = e.sheet;
            let at = match e.sel {
                Sel::Complex(c) => w.complex(c.comps, At::default()),
                Sel::Parent => At::default(),
            };
            // the parent rules' compounds are further left: past the
            // combinator a relative selector ends in, or where `&` is,
            // taken as the leftmost compound walked
            let nested = match e.sel {
                Sel::Complex(c) => c.relative || has_nesting(c.comps),
                Sel::Parent => true,
            };
            if nested {
                w.parents(e.parent, at);
            }
            w.f.reval.push(w.reval);
        }
        w.f
    }

    /// How far a change to the class or id named `key` reaches.
    pub fn class_tier(&self, key: u64) -> Tier {
        self.class.get(&key).copied().unwrap_or_default()
    }

    pub fn id_tier(&self, key: u64) -> Tier {
        self.id.get(&key).copied().unwrap_or_default()
    }

    pub fn attr_tier(&self, lower: &str) -> Tier {
        self.attr.get(&name_key(lower)).copied().unwrap_or_default()
    }

    pub fn pseudo_tier(&self, p: PseudoClass) -> Tier {
        self.pseudo.get(&p).copied().unwrap_or_default()
    }
}
