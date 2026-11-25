//! What roles, names and states are computed from: the DOM, its
//! computed values when there are any, and the attribute indexes.

use madcawl_css::style::{Display, Styles};
use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_str::atom;

use crate::index::AttrIndex;
use crate::text::attr_string;

pub(crate) struct Cx<'a> {
    pub dom: &'a Dom,
    pub styles: Option<&'a Styles>,
    pub ids: &'a AttrIndex,
    pub label_for: &'a AttrIndex,
}

impl Cx<'_> {
    /// Hidden from a reader: `aria-hidden="true"` on it or above it, or
    /// not rendered, or `visibility` other than visible.
    pub fn hidden(&self, n: NodeId) -> bool {
        if self.styles.is_some_and(|s| !s.is_visible(self.dom, n)) {
            return true;
        }
        let mut up = Some(n);
        while let Some(p) = up {
            if self.aria_hidden(p) {
                return true;
            }
            up = self.dom.parent(p);
        }
        false
    }

    /// Hidden by `visibility` alone: rendered, and not `aria-hidden`.
    pub fn only_invisible(&self, n: NodeId) -> bool {
        self.styles.is_some_and(|s| s.is_rendered(self.dom, n)) && {
            let mut up = Some(n);
            let mut aria = false;
            while let Some(p) = up {
                aria |= self.aria_hidden(p);
                up = self.dom.parent(p);
            }
            !aria
        }
    }

    pub fn aria_hidden(&self, n: NodeId) -> bool {
        self.dom.kind(n) == NodeKind::Element
            && attr_string(self.dom, n, atom!("aria-hidden"))
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
    }

    /// The element's box is block-level, so its text stands apart from
    /// its neighbours' in a name.
    pub fn block_level(&self, n: NodeId) -> bool {
        let Some(s) = self.styles.and_then(|s| s.get(n)) else {
            return false;
        };
        !matches!(
            s.display,
            Display::Inline
                | Display::Contents
                | Display::None
                | Display::Ruby
                | Display::WebkitInlineBox
        )
    }
}
