//! The ParentNode mixin's selector queries, shared by Document,
//! DocumentFragment and Element.

use madcawl_dom::NodeId;
use madcawl_js::Engine;
use madcawl_str::DomString;

use crate::convert::{Thrown, throw_dom};
use crate::host::DomHost;

fn matching<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, selectors: &DomString, limit: usize) -> Result<Vec<NodeId>, Thrown> {
    let mut b = Vec::new();
    selectors.write_wtf8(&mut b);
    let text = String::from_utf8_lossy(&b);
    madcawl_css::style::query(&h.dom, this, &text, limit)
        .map_err(|_| throw_dom(e, h, "SyntaxError", "not a valid selector list"))
}

pub(crate) fn query_selector<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, selectors: DomString) -> Result<Option<NodeId>, Thrown> {
    Ok(matching(e, h, this, &selectors, 1)?.into_iter().next())
}

/// A fixed NodeList of what the selectors match.
pub(crate) fn query_selector_all<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, selectors: DomString) -> Result<crate::side::SideRef, Thrown> {
    let found = matching(e, h, this, &selectors, usize::MAX)?;
    Ok(h.fixed_list(found))
}
