//! Which nodes a reader may show: what the cascade renders and leaves
//! `visibility: visible`, and not at or under `opacity: 0`.

use madcawl_css::style::Styles;
use madcawl_dom::{Dom, NodeId, NodeKind};

pub struct Shown<'a> {
    pub dom: &'a Dom,
    pub styles: &'a Styles,
    /// By node index: the element or an ancestor is fully transparent
    clear: Vec<bool>,
}

impl<'a> Shown<'a> {
    pub fn new(dom: &'a Dom, styles: &'a Styles) -> Self {
        let mut clear = Vec::new();
        for n in dom.descendants(dom.document()) {
            if dom.kind(n) != NodeKind::Element {
                continue;
            }
            let i = n.index() as usize;
            if clear.len() <= i {
                clear.resize(i + 1, false);
            }
            let above = dom
                .parent(n)
                .is_some_and(|p| clear.get(p.index() as usize).copied().unwrap_or(false));
            clear[i] = above || styles.get(n).is_some_and(|s| s.opacity() == 0.0);
        }
        Shown { dom, styles, clear }
    }

    fn transparent(&self, el: NodeId) -> bool {
        self.clear
            .get(el.index() as usize)
            .copied()
            .unwrap_or(false)
    }

    fn element_of(&self, n: NodeId) -> Option<NodeId> {
        match self.dom.kind(n) {
            NodeKind::Element => Some(n),
            _ => self.dom.parent(n),
        }
    }

    /// Something at or below it may be shown: it makes boxes and is not
    /// transparent.
    pub fn rendered(&self, n: NodeId) -> bool {
        self.styles.is_rendered(self.dom, n)
            && !self.element_of(n).is_some_and(|e| self.transparent(e))
    }

    /// The element, or the text node, is shown.
    pub fn shown(&self, n: NodeId) -> bool {
        self.styles.is_visible(self.dom, n)
            && !self.element_of(n).is_some_and(|e| self.transparent(e))
    }
}
