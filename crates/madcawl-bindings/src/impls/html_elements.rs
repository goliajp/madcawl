//! The members of the element interfaces below HTMLElement other than
//! HTMLScriptElement's: reflected content attributes.

/// Each function pair reflects a `DOMString` attribute, or with `url`
/// one that holds a URL.
macro_rules! reflect {
    ($get:ident, $set:ident, $name:literal) => {
        reflect!($get, $set, $name, string);
    };
    ($get:ident, $set:ident, $name:literal, $read:ident) => {
        pub(crate) fn $get<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
            Ok(super::super::reflect::$read(h, this, atom!($name)))
        }

        pub(crate) fn $set<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, v: DomString) -> Result<(), Thrown> {
            super::super::attributes::set(e, h, this, atom!($name), &v)
        }
    };
}

pub(crate) mod anchor {
    use madcawl_dom::NodeId;
    use madcawl_js::Engine;
    use madcawl_str::{DomString, atom};

    use crate::convert::Thrown;
    use crate::host::DomHost;

    reflect!(target, set_target, "target");
    reflect!(rel, set_rel, "rel");
    reflect!(href, set_href, "href", url);
}

pub(crate) mod link {
    use madcawl_dom::NodeId;
    use madcawl_js::Engine;
    use madcawl_str::{DomString, atom};

    use crate::convert::Thrown;
    use crate::host::DomHost;

    reflect!(href, set_href, "href", url);
    reflect!(rel, set_rel, "rel");
}

pub(crate) mod image {
    use madcawl_dom::NodeId;
    use madcawl_js::Engine;
    use madcawl_str::{DomString, atom};

    use crate::convert::Thrown;
    use crate::host::DomHost;

    reflect!(alt, set_alt, "alt");
    reflect!(src, set_src, "src", url);
}

pub(crate) mod iframe {
    use madcawl_dom::NodeId;
    use madcawl_js::Engine;
    use madcawl_str::{DomString, atom};

    use crate::convert::Thrown;
    use crate::host::DomHost;

    reflect!(src, set_src, "src", url);
    reflect!(srcdoc, set_srcdoc, "srcdoc");
    reflect!(name, set_name, "name");
    reflect!(width, set_width, "width");
    reflect!(height, set_height, "height");

    /// The child navigable's document, when its origin is the iframe's
    /// document's (`frames.rs`).
    pub(crate) fn content_document<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<NodeId>, Thrown> {
        Ok(h.content_document(this))
    }

    pub(crate) fn content_window<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<crate::convert::Any<E>>, Thrown> {
        Ok(h.content_window(e, this).map(crate::convert::Any))
    }
}
