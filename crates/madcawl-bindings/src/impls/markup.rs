//! `innerHTML` and `outerHTML`: the element's markup written by the
//! fragment serializing algorithm, and markup parsed by the fragment
//! parsing algorithm put in place of its children or of itself.

use madcawl_dom::{HeldNode, NodeId, NodeKind};
use madcawl_js::Engine;
use madcawl_str::{DomString, atom};

use crate::convert::{Thrown, throw_dom};
use crate::host::DomHost;

/// Whether scripting is enabled for `n`: not in a template's contents,
/// whose document has no browsing context.
fn scripting<E: Engine>(h: &DomHost<E>, n: NodeId) -> bool {
    !h.dom.is_inert_document(h.dom.owner_document(n))
}

pub(crate) fn inner_html<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    let mut b = Vec::new();
    h.dom.write_html(this, scripting(h, this), &mut b);
    Ok(DomString::from_wtf8(&b))
}

pub(crate) fn outer_html<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    let mut b = Vec::new();
    h.dom.write_outer_html(this, scripting(h, this), &mut b);
    Ok(DomString::from_wtf8(&b))
}

/// `markup` parsed for `context`: the fragment, held, its script
/// elements already started.
fn parse<E: Engine>(e: &mut E, h: &mut DomHost<E>, context: NodeId, markup: &DomString) -> Result<HeldNode, Thrown> {
    let mut b = Vec::new();
    markup.write_wtf8(&mut b);
    let s = scripting(h, context);
    let Some(parsed) = h.dom.parse_fragment(context, &b, s) else {
        return Err(super::document::full(e, h));
    };
    let f = h.dom.hold(parsed.fragment);
    if !parsed.complete {
        h.dom.let_go(f);
        h.settle(e);
        return Err(super::attributes::full(e, h));
    }
    mark_started(h, f.id());
    Ok(f)
}

/// Mark the script elements below `root` already started, those in
/// template contents too.
pub(crate) fn mark_started<E: Engine>(h: &mut DomHost<E>, root: NodeId) {
    let mut roots = vec![root];
    while let Some(r) = roots.pop() {
        for n in h.dom.descendants(r) {
            if crate::scripts::is_script(&h.dom, n) {
                let flags = crate::scripts::Flags { started: true, ..h.scripts.flags(n) };
                h.scripts.flags.insert(n.index(), flags);
            } else if h.dom.kind(n) == NodeKind::Element {
                roots.extend(h.dom.contents(n));
            }
        }
    }
}

/// Move the children of fragment `f` into `parent` before `before`, then
/// give `f` back: the nodes moved.
fn insert_all<E: Engine>(h: &mut DomHost<E>, f: HeldNode, parent: NodeId, before: Option<NodeId>) -> Vec<NodeId> {
    let nodes: Vec<NodeId> = h.dom.children(f.id()).collect();
    for &n in &nodes {
        h.dom.insert_node(parent, n, before);
    }
    h.dom.let_go(f);
    nodes
}

/// Parse `markup` for this element and replace its children (a
/// template's contents' children) with the result.
pub(crate) fn set_inner_html<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, markup: DomString) -> Result<(), Thrown> {
    let f = parse(e, h, this, &markup)?;
    let target = h.dom.contents(this).unwrap_or(this);
    while let Some(c) = h.dom.first_child(target) {
        h.dom.remove_node(c);
    }
    let nodes = insert_all(h, f, target, None);
    // a script element given children may run, and the iframes get
    // navigables
    h.nodes_inserted(e, target, &nodes);
    Ok(())
}

/// Parse `markup` for this element's parent (a `body` for a fragment)
/// and replace the element with the result; nothing without a parent.
pub(crate) fn set_outer_html<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, markup: DomString) -> Result<(), Thrown> {
    let Some(parent) = h.dom.parent(this) else { return Ok(()) };
    let f = match h.dom.kind(parent) {
        NodeKind::Document => {
            return Err(throw_dom(e, h, "NoModificationAllowedError", "the element is the document's child"));
        }
        NodeKind::Fragment => {
            let doc = h.dom.owner_document(this);
            let Some(body) = h.dom.new_element_in(doc, atom!("body")) else {
                return Err(super::attributes::full(e, h));
            };
            let held = h.dom.hold(body);
            let f = parse(e, h, body, &markup);
            h.dom.let_go(held);
            f?
        }
        _ => parse(e, h, parent, &markup)?,
    };
    // the element is still the parent's child: parsing runs no script
    let before = h.dom.next_sibling(this);
    h.dom.remove_node(this);
    let nodes = insert_all(h, f, parent, before);
    h.nodes_inserted(e, parent, &nodes);
    Ok(())
}
