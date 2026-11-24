//! What is not rendered whatever the computed values say: the content
//! of a closed `<details>` but its summary, the fallback content of
//! elements that draw something else, and the SVG elements that are
//! only ever referenced.

use madcawl_dom::NodeId;
use madcawl_html::Namespace;
use madcawl_str::{Atom, atom};

use super::matching::Matcher;
use super::values::{ComputedStyle, ContentVisibility, Display};
use super::{CHILDREN_HIDDEN, CONTENTS_SKIPPED, NOT_RENDERED};

impl Matcher<'_, '_> {
    /// The flags `el` has from its parent `p`, whose values are `ps` and
    /// flags `f`.
    pub(crate) fn flags_below(
        &self,
        p: NodeId,
        ps: Option<&ComputedStyle>,
        f: u8,
        el: NodeId,
    ) -> u8 {
        let hides = ps.is_some_and(|ps| {
            ps.content_visibility == ContentVisibility::Hidden && ps.display.can_contain_size()
        }) || f & CHILDREN_HIDDEN != 0 && !self.shown_summary(p, el);
        f & !CHILDREN_HIDDEN | if hides { CONTENTS_SKIPPED } else { 0 }
    }

    /// The flags `el` has of itself, its computed `display` being
    /// `display`.
    pub(crate) fn own_flags(&self, el: NodeId, display: Display) -> u8 {
        let none = display == Display::None
            || display == Display::Contents && self.contents_is_none(el)
            || self.never_rendered(el);
        let mut f = if none { NOT_RENDERED } else { 0 };
        if self.hides_children(el) {
            f |= CHILDREN_HIDDEN;
        }
        f
    }

    fn named(&self, el: NodeId, ns: Namespace, names: &[Atom]) -> bool {
        self.dom.namespace(el) == Some(ns)
            && self.dom.local_name(el).is_some_and(|n| names.contains(&n))
    }

    /// Its children are not rendered (but a closed details' summary).
    pub(crate) fn hides_children(&self, el: NodeId) -> bool {
        let closed = self.named(el, Namespace::Html, &[atom!("details")])
            && self.dom.attribute(el, atom!("open")).is_none();
        closed
            || self.named(
                el,
                Namespace::Html,
                &[
                    atom!("audio"),
                    atom!("canvas"),
                    atom!("iframe"),
                    atom!("video"),
                ],
            )
    }

    /// `child` is the summary a closed details shows.
    pub(crate) fn shown_summary(&self, parent: NodeId, child: NodeId) -> bool {
        self.named(parent, Namespace::Html, &[atom!("details")])
            && self
                .dom
                .children(parent)
                .find(|&c| self.named(c, Namespace::Html, &[atom!("summary")]))
                == Some(child)
    }

    /// `display: contents` on it behaves as `display: none`: the
    /// replaced elements and form controls (HTML, "Rendering").
    pub(crate) fn contents_is_none(&self, el: NodeId) -> bool {
        self.named(
            el,
            Namespace::Html,
            &[
                atom!("audio"),
                atom!("br"),
                atom!("canvas"),
                atom!("embed"),
                atom!("frame"),
                atom!("frameset"),
                atom!("iframe"),
                atom!("img"),
                atom!("input"),
                atom!("meter"),
                atom!("object"),
                atom!("progress"),
                atom!("select"),
                atom!("textarea"),
                atom!("video"),
                atom!("wbr"),
            ],
        )
    }

    /// An SVG element that is never rendered where it is.
    pub(crate) fn never_rendered(&self, el: NodeId) -> bool {
        self.named(
            el,
            Namespace::Svg,
            &[
                atom!("clipPath"),
                atom!("defs"),
                atom!("desc"),
                atom!("filter"),
                atom!("linearGradient"),
                atom!("marker"),
                atom!("mask"),
                atom!("metadata"),
                atom!("pattern"),
                atom!("radialGradient"),
                atom!("script"),
                atom!("style"),
                atom!("symbol"),
                atom!("title"),
            ],
        )
    }
}
