#![allow(dead_code)]

use madcawl_css::style::{Device, Styles, style_document};
use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_html::{DocumentParser, Options, Source, Step};
use madcawl_layout::{Layout, Rect};
use madcawl_str::atom;

pub fn parse(html: &str) -> Dom {
    let mut p = DocumentParser::new(Dom::new, Options::default(), Source::default());
    p.feed(html.as_bytes());
    p.end();
    assert_eq!(p.run(), Step::Done);
    p.into_sink()
}

pub fn style(dom: &Dom) -> Styles {
    style_document(dom, Device::default(), &mut |_| None)
}

/// The element whose `id` is `id`.
pub fn by_id(dom: &Dom, id: &str) -> NodeId {
    dom.descendants(dom.document())
        .find(|&n| {
            dom.kind(n) == NodeKind::Element
                && dom.attribute(n, atom!("id")).and_then(|v| v.as_ascii_str()) == Some(id)
        })
        .unwrap_or_else(|| panic!("no #{id}"))
}

/// x, y, width, height in px.
pub fn px(r: Rect) -> [f32; 4] {
    [r.x.px(), r.y.px(), r.w.px(), r.h.px()]
}

pub fn rect(l: &mut Layout, dom: &Dom, id: &str) -> [f32; 4] {
    px(l.border_box(by_id(dom, id))
        .unwrap_or_else(|| panic!("#{id} has no box")))
}

/// The border box of every element, by its id or else its place in document order.
pub fn all_rects(l: &mut Layout, dom: &Dom) -> Vec<(String, Option<[f32; 4]>)> {
    let els: Vec<NodeId> = dom
        .descendants(dom.document())
        .filter(|&n| dom.kind(n) == NodeKind::Element)
        .collect();
    els.into_iter()
        .enumerate()
        .map(|(k, n)| {
            let id = dom.attribute(n, atom!("id")).and_then(|v| v.as_ascii_str());
            let name = id.map_or_else(|| format!("element {k}"), |i| format!("#{i}"));
            (name, l.border_box(n).map(px))
        })
        .collect()
}

/// Asserts that reusing last results gives every element the border box it has when every box is laid out
/// every time.
pub fn same_without_reuse(html: &str, width: f32, height: f32) {
    let dom = parse(html);
    let st = style(&dom);
    let mut on = Layout::new(&dom, &st, width, height);
    let mut off = Layout::new(&dom, &st, width, height);
    off.set_reuse(false);
    let (a, b) = (all_rects(&mut on, &dom), all_rects(&mut off, &dom));
    // a probe that lays nothing out, or reuses anyway, would compare nothing
    let c = off.counters();
    assert!(c.laid > 0 && c.reused == 0, "{c:?} without reuse in {html}");
    for ((id, x), (_, y)) in a.iter().zip(&b) {
        assert_eq!(x, y, "{id} with reuse, then without, in {html}");
    }
}
