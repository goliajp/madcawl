//! The kind of box an element generates.

use madcawl_css::style::{ComputedStyle, Display};
use madcawl_dom::{Dom, NodeId};
use madcawl_html::Namespace;
use madcawl_str::{Atom, atom};

use super::Kind;

const REPLACED: [Atom; 9] = [
    atom!("img"),
    atom!("iframe"),
    atom!("video"),
    atom!("canvas"),
    atom!("embed"),
    atom!("input"),
    atom!("textarea"),
    atom!("select"),
    atom!("object"),
];

pub(super) fn is_html(dom: &Dom, n: NodeId, names: &[Atom]) -> bool {
    dom.namespace(n) == Some(Namespace::Html)
        && dom.local_name(n).is_some_and(|l| names.contains(&l))
}

pub(super) fn kind_of(dom: &Dom, n: NodeId, s: &ComputedStyle) -> Kind {
    let inline = {
        use Display::*;
        matches!(
            s.display,
            Inline
                | InlineBlock
                | InlineListItem
                | InlineTable
                | InlineFlex
                | InlineGrid
                | Ruby
                | WebkitInlineBox
        )
    };
    let svg_root = dom.namespace(n) == Some(Namespace::Svg)
        && dom
            .parent(n)
            .is_none_or(|p| dom.namespace(p) != Some(Namespace::Svg));
    if is_html(dom, n, &REPLACED) || svg_root {
        Kind::Replaced { inline }
    } else if is_html(dom, n, &[atom!("br"), atom!("wbr")]) {
        Kind::Break
    } else if matches!(s.display, Display::Inline | Display::Ruby) {
        Kind::Inline
    } else if inline {
        Kind::InlineBlock
    } else {
        Kind::Block
    }
}
