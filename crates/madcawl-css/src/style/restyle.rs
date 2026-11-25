//! Restyling after the document changes. A change to an element's
//! class, id, attribute or state restyles what `Features` says the
//! feature can reach: the element, it and its later siblings, or its
//! subtree; a feature in `:has()` restyles the document. An element
//! whose values change restyles its children too, since they inherit
//! from it. An element the parser appends is styled, and so is its
//! parent when a selector asks whether it is empty, the element sibling
//! before it when one asks what is last, and every earlier sibling for
//! `:nth-last-child()`. Sibling ranges are walked once per parent, from
//! the earliest change or up to the latest append, and each append
//! looks back past the others of the same restyle at most once, so many
//! changes among many siblings stay linear; the siblings walked count
//! as visits.

use std::collections::HashMap;

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_str::{Atom, atom};

use crate::selector::PseudoClass;

use super::Cascade;
use super::features::{Features, Tier, name_key};
use super::matching::text;
use super::roots::{Appends, Roots, depth, reach};
use super::states::ElementStates;
use super::styles::Styles;

/// A change to the document that may change computed values.
#[derive(Clone, Debug)]
pub enum Change {
    /// The attribute `name` (no namespace) of `el` was set or removed;
    /// `old` is its value before, `None` when it had none
    Attribute {
        el: NodeId,
        name: Atom,
        old: Option<String>,
    },
    /// The host set or cleared `pseudo` on `el` in its `ElementStates`
    State { el: NodeId, pseudo: PseudoClass },
    /// The parser appended `node`, and what is below it, as the last
    /// child of its parent. The appends of one restyle are the last
    /// children of their parents, every one of them listed, in order
    Appended(NodeId),
}

/// What a restyle did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Restyled {
    /// Elements styled again
    pub styled: u64,
    /// Of those, the ones whose values, custom properties or flags changed
    pub changed: u64,
    /// Elements taken from the work list, styled or skipped as done:
    /// the work a restyle did, linear in the elements it reaches
    pub visited: u64,
    /// The whole document was styled again
    pub full: bool,
}

/// Attributes some pseudo-class reads: a change to one reaches as far
/// as any pseudo-class does. `disabled` on a fieldset, `lang` and `dir`
/// also reach the descendants.
const STATEFUL: &[&str] = &[
    "checked",
    "dir",
    "disabled",
    "href",
    "lang",
    "max",
    "min",
    "multiple",
    "open",
    "placeholder",
    "readonly",
    "required",
    "selected",
    "type",
    "value",
];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Mode {
    Own = 1,
    Subtree = 2,
}

fn words(v: &str) -> impl Iterator<Item = &str> {
    v.split(['\t', '\n', '\x0c', '\r', ' '])
        .filter(|w| !w.is_empty())
}

fn attribute_tier(f: &Features, dom: &Dom, el: NodeId, name: Atom, old: Option<&str>) -> Tier {
    let now = dom.attribute(el, name).map(text);
    let now = now.as_deref();
    let lower = text(&name.to_dom_string()).to_ascii_lowercase();
    // the style attribute and presentational hints: the element itself
    let mut t = Tier::OWN.or(f.attr_tier(&lower));
    if name == atom!("class") {
        for w in words(old.unwrap_or("")).chain(words(now.unwrap_or(""))) {
            t = t.or(f.class_tier(name_key(w)));
        }
    } else if name == atom!("id") {
        for w in old.into_iter().chain(now) {
            t = t.or(f.id_tier(name_key(w)));
        }
    }
    if STATEFUL.contains(&lower.as_str()) {
        let any = f.pseudo.values().fold(Tier::NONE, |a, &b| a.or(b));
        t = t.or(any);
        let below = matches!(lower.as_str(), "disabled" | "lang" | "dir") && !any.is_none()
            || matches!(lower.as_str(), "lang" | "dir") && !f.lang_dir.is_none();
        if below {
            t = t.or(Tier::SUBTREE);
        }
    }
    t
}

impl Cascade<'_> {
    /// Bring `styles`, made by this cascade for `dom`, up to date after
    /// `changes`, with the elements in the states `states` gives. Styles
    /// made by another cascade or in another quirks mode, or whose
    /// `PathKey` tables have grown past twice the document in keys or in
    /// bytes, or whose pseudo-element memo or custom property sets past
    /// twice its nodes, are made again in full.
    pub fn restyle(
        &self,
        dom: &Dom,
        styles: &mut Styles,
        states: &ElementStates,
        changes: &[Change],
    ) -> Restyled {
        styles.version = super::styles::Version::next();
        let bound = 2 * dom.node_count() as usize + 1024;
        // each key may hold a whole style attribute, so a count bound
        // alone lets a few elements whose style keeps changing hold the
        // document many times over
        let held = 2 * dom.bytes() + (1 << 20);
        let stale = styles.generation != self.generation || styles.quirks != super::is_quirks(dom);
        let memo = styles.pseudo_memo.len() > bound || styles.sets.len() > bound;
        if stale || memo || styles.path_keys() > bound || styles.path_key_bytes() > held {
            return self.full(dom, styles, states);
        }
        let mut run = self.run(dom, states);
        let f = &run.prep.features;
        let mut roots = Roots::default();
        let mut appends = Appends::default();
        let mut full = false;
        for c in changes {
            full |= match c {
                Change::Attribute { el, name, old } => {
                    let t = attribute_tier(f, dom, *el, *name, old.as_deref());
                    reach(dom, *el, t, &mut roots)
                }
                Change::State { el, pseudo } => {
                    let within = *pseudo == PseudoClass::Focus
                        && !f.pseudo_tier(PseudoClass::FocusWithin).is_none();
                    within || reach(dom, *el, f.pseudo_tier(*pseudo), &mut roots)
                }
                Change::Appended(n) => appends.add(f, dom, *n, &mut roots),
            };
        }
        appends.check(dom);
        if full {
            return self.full(dom, styles, states);
        }
        roots.expand(dom);
        let mut r = Restyled {
            visited: roots.steps,
            ..Restyled::default()
        };
        let mut roots = roots.list;
        // a restyle is a run of its own and gets the whole `var()`
        // budget again: the budget bounds the work of one run, and the
        // elements a restyle reaches are at most the document
        styles.memo.new_run();
        styles.stats = Default::default();
        let root_font = styles.root_font;
        // parents before children, so each is styled against its
        // parent's new values
        roots.sort_by_cached_key(|&(n, m)| (depth(dom, n), std::cmp::Reverse(m)));
        let mut done: HashMap<NodeId, Mode> = HashMap::new();
        let mut stack = Vec::new();
        for (root, mode) in roots {
            stack.push((root, mode));
            while let Some((n, mode)) = stack.pop() {
                r.visited += 1;
                let seen = done.get(&n).copied();
                if seen.is_some_and(|s| s >= mode) || !dom.is_connected(n) {
                    continue;
                }
                let (changed, pseudo) = if seen.is_none() {
                    r.styled += 1;
                    run.style_one(n, styles)
                } else {
                    (false, false)
                };
                // a pseudo-element's values reach no child's
                r.changed += u64::from(changed || pseudo);
                done.insert(n, mode);
                let child = match mode {
                    Mode::Subtree => Some(Mode::Subtree),
                    Mode::Own => changed.then_some(Mode::Own),
                };
                if let Some(m) = child {
                    for c in dom
                        .children(n)
                        .filter(|&c| dom.kind(c) == NodeKind::Element)
                    {
                        stack.push((c, m));
                    }
                }
            }
        }
        if styles.root_font != root_font {
            // every rem was of the old size
            return self.full(dom, styles, states);
        }
        r
    }

    fn full(&self, dom: &Dom, styles: &mut Styles, states: &ElementStates) -> Restyled {
        *styles = self.compute_with(dom, states);
        Restyled {
            styled: styles.stats().elements,
            changed: styles.stats().elements,
            visited: styles.stats().elements,
            full: true,
        }
    }
}
