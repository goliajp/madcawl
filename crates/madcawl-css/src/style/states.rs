//! What a person or script does to elements that selectors can see:
//! hovering, pressing, focusing, targeting. Each element's own flags;
//! a host that hovers an element sets `:hover` on it and on each of its
//! ancestors, the chain HTML says is hovered.

use std::collections::HashMap;

use madcawl_dom::{Dom, NodeId};

use crate::selector::PseudoClass;

/// The flag of each pseudo-class a host may set.
fn bit(p: PseudoClass) -> Option<u8> {
    Some(match p {
        PseudoClass::Hover => 1,
        PseudoClass::Active => 2,
        PseudoClass::Focus => 4,
        PseudoClass::FocusVisible => 8,
        PseudoClass::Target => 16,
        _ => return None,
    })
}

#[derive(Clone, Debug, Default)]
pub struct ElementStates {
    /// An element without flags has no entry
    flags: HashMap<NodeId, u8>,
}

impl ElementStates {
    pub fn new() -> Self {
        ElementStates::default()
    }

    /// Set or clear `p` on `el`; false when `p` is not one a host sets.
    pub fn set(&mut self, el: NodeId, p: PseudoClass, on: bool) -> bool {
        let Some(b) = bit(p) else { return false };
        let f = self.flags.entry(el).or_default();
        if on {
            *f |= b;
        } else {
            *f &= !b;
        }
        if *f == 0 {
            self.flags.remove(&el);
        }
        true
    }

    pub fn has(&self, el: NodeId, p: PseudoClass) -> bool {
        bit(p).is_some_and(|b| self.flags.get(&el).is_some_and(|f| f & b != 0))
    }

    /// The elements that are focused.
    pub fn focused(&self) -> impl Iterator<Item = NodeId> + '_ {
        let b = bit(PseudoClass::Focus).expect("a host flag");
        self.flags
            .iter()
            .filter(move |(_, f)| *f & b != 0)
            .map(|(&n, _)| n)
    }

    /// `el` or an element below it is focused.
    pub fn focus_within(&self, dom: &Dom, el: NodeId) -> bool {
        self.focused()
            .any(|f| std::iter::successors(Some(f), |&x| dom.parent(x)).any(|x| x == el))
    }

    pub fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }
}
