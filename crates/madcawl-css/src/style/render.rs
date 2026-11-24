//! What is not rendered whatever the computed values say: the content
//! of a closed `<details>` but its summary, the fallback content of
//! elements that draw something else, and the SVG elements that are
//! only ever referenced.

use madcawl_dom::NodeId;
use madcawl_html::Namespace;
use madcawl_str::{Atom, atom};

use super::matching::Matcher;

impl Matcher<'_, '_> {
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
