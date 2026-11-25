//! Pages parsed, styled, laid out and painted for the tests.

#![allow(dead_code)]

use madcawl_css::style::{Device, Styles, style_document};
use madcawl_dom::Dom;
use madcawl_html::{DocumentParser, Options, Source, Step};
use madcawl_layout::{FragKind, Layout};
use madcawl_paint::{Bitmap, DisplayList, Item, Raster, RectF, paint};

pub const VIEW: RectF = RectF {
    x: 0.0,
    y: 0.0,
    w: 200.0,
    h: 100.0,
};

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

pub fn list_of(html: &str) -> DisplayList {
    let dom = parse(html);
    let styles = style(&dom);
    let mut l = Layout::new(&dom, &styles, VIEW.w, VIEW.h);
    paint(&mut l, &dom, &styles, None)
}

pub fn raster(html: &str) -> Bitmap {
    let dom = parse(html);
    let styles = style(&dom);
    let mut l = Layout::new(&dom, &styles, VIEW.w, VIEW.h);
    let list = paint(&mut l, &dom, &styles, Some(VIEW));
    Raster {
        view: VIEW,
        scale: 1.0,
    }
    .draw(&list, &mut l.shaper().fonts)
}

/// The list and the page x of every text run fragment, in tree order.
pub fn list_and_runs(html: &str) -> (DisplayList, Vec<f32>) {
    let dom = parse(html);
    let styles = style(&dom);
    let mut l = Layout::new(&dom, &styles, VIEW.w, VIEW.h);
    let mut xs = Vec::new();
    let root = l.fragments().cloned().expect("a fragment tree");
    root.frag.walk(|at, f| {
        if f.kind == FragKind::Text && f.size.w.px() > 0.0 {
            xs.push((root.offset + at).x.px());
        }
    });
    (paint(&mut l, &dom, &styles, None), xs)
}

/// The red channel of every solid rectangle, in paint order.
pub fn rect_reds(list: &DisplayList) -> Vec<u8> {
    list.entries
        .iter()
        .filter_map(|e| match e.item {
            Item::Rect { color, .. } => Some(color.0[0]),
            _ => None,
        })
        .collect()
}

/// Where the first rectangle with red channel `red` is in the list.
pub fn rect_at(list: &DisplayList, red: u8) -> usize {
    list.entries
        .iter()
        .position(|e| matches!(e.item, Item::Rect { color, .. } if color.0[0] == red))
        .unwrap_or_else(|| panic!("a rectangle of red {red}"))
}

/// Where each glyph run is in the list.
pub fn glyph_runs(list: &DisplayList) -> Vec<usize> {
    (0..list.entries.len())
        .filter(|&k| matches!(list.entries[k].item, Item::Glyphs { .. }))
        .collect()
}
