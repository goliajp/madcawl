//! Document members of the HTML Standard about the document as a whole:
//! `readyState`, `title` and `defaultView`.

use madcawl_dom::{NodeId, NodeKind};
use madcawl_html::Namespace;
use madcawl_js::Engine;
use madcawl_str::{DomString, atom};

use crate::convert::{Any, Thrown};
use crate::host::DomHost;
use crate::scripts::Phase;

/// `loading` while the parser runs, `interactive` once it is done, and
/// `complete` from the window's `load` on; a document no parser built
/// is complete.
pub(crate) fn ready_state<E: Engine>(_: &mut E, h: &mut DomHost<E>, _this: NodeId) -> Result<DomString, Thrown> {
    let s = match h.scripts.phase {
        Phase::Parsing => "loading",
        Phase::Idle | Phase::Complete => "complete",
        _ => "interactive",
    };
    Ok(DomString::from_wtf8(s.as_bytes()))
}

/// The window, for the window's own document; null for any other.
pub(crate) fn default_view<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Option<Any<E>>, Thrown> {
    Ok((this == h.dom.document()).then(|| Any(e.global(h.realm))))
}

fn root(h: &DomHost<impl Engine>, doc: NodeId) -> Option<NodeId> {
    h.dom.children(doc).find(|&c| h.dom.kind(c) == NodeKind::Element)
}

fn is(h: &DomHost<impl Engine>, n: NodeId, ns: Namespace, name: madcawl_str::Atom) -> bool {
    h.dom.namespace(n) == Some(ns) && h.dom.local_name(n) == Some(name)
}

/// The document element when it is an SVG `svg` element.
fn svg_root(h: &DomHost<impl Engine>, doc: NodeId) -> Option<NodeId> {
    root(h, doc).filter(|&r| is(h, r, Namespace::Svg, atom!("svg")))
}

/// The title element: the SVG `title` child of an `svg` document
/// element, otherwise the first HTML `title` element in tree order.
fn title_element(h: &DomHost<impl Engine>, doc: NodeId) -> Option<NodeId> {
    match svg_root(h, doc) {
        Some(r) => h.dom.children(r).find(|&c| is(h, c, Namespace::Svg, atom!("title"))),
        None => h.dom.descendants(doc).find(|&n| is(h, n, Namespace::Html, atom!("title"))),
    }
}

fn is_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | 0x0C | b'\r' | b' ')
}

/// The title element's child text, ASCII whitespace stripped and
/// collapsed.
pub(crate) fn title<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    let Some(t) = title_element(h, this) else { return Ok(DomString::empty()) };
    let mut b = Vec::new();
    for c in h.dom.children(t) {
        if h.dom.kind(c) == NodeKind::Text {
            h.dom.text(c).expect("a text node").write_wtf8(&mut b);
        }
    }
    let words: Vec<&[u8]> = b.split(|&c| is_space(c)).filter(|w| !w.is_empty()).collect();
    Ok(DomString::from_wtf8(&words.join(&b' ')))
}

/// The title element's children replaced by the text; one is made, as
/// the first child of an `svg` document element or the last of the
/// head, when there is none; nothing changes in a document with neither.
pub(crate) fn set_title<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, v: DomString) -> Result<(), Thrown> {
    if svg_root(h, this).is_none() && !root(h, this).is_some_and(|r| h.dom.in_html_namespace(r)) {
        return Ok(());
    }
    let t = match title_element(h, this) {
        Some(t) => t,
        None => {
            let (ns, parent) = match svg_root(h, this) {
                Some(r) => (Namespace::Svg, r),
                None => match super::document::head(e, h, this)? {
                    Some(head) => (Namespace::Html, head),
                    None => return Ok(()),
                },
            };
            let Some(t) = h.dom.new_element_ns(ns, b"title") else { return Err(super::document::full(e, h)) };
            let first = if ns == Namespace::Svg { h.dom.first_child(parent) } else { None };
            super::node::insert_before(e, h, parent, t, first)?;
            t
        }
    };
    super::node::set_text_content(e, h, t, Some(v))
}
