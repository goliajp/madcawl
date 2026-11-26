//! The ParentNode mixin's queries, shared by Document and Element.

use madcawl_dom::{NodeId, NodeKind};
use madcawl_js::Engine;
use madcawl_str::DomString;

use crate::convert::{StaticList, Thrown, throw_dom};
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

pub(crate) fn query_selector_all<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, selectors: DomString) -> Result<StaticList, Thrown> {
    matching(e, h, this, &selectors, usize::MAX).map(StaticList)
}

/// The descendant elements whose qualified name is `name` (`*` for all),
/// compared in ASCII lower case for HTML elements.
pub(crate) fn get_elements_by_tag_name<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId, name: DomString) -> Result<StaticList, Thrown> {
    let dom = &h.dom;
    let mut want = Vec::new();
    name.write_wtf8(&mut want);
    let lower = want.to_ascii_lowercase();
    let mut local = Vec::new();
    let found = dom
        .descendants(this)
        .filter(|&n| dom.kind(n) == NodeKind::Element)
        .filter(|&n| {
            if want == b"*" {
                return true;
            }
            local.clear();
            dom.local_name(n).expect("an element").write_wtf8(&mut local);
            local == *if dom.in_html_namespace(n) { &lower } else { &want }
        })
        .collect();
    Ok(StaticList(found))
}
