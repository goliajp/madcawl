mod common;

use common::*;
use madcawl_dom::NodeId;
use madcawl_layout::{Au, Layout, MAX_DEPTH};
use madcawl_str::DomString;

const PAGE: &str =
    "<!doctype html><body><p id=p>Some <b>text</b> to lay out<div id=d style='width:50%'>x</div>";

#[test]
fn a_windowless_page_lays_out_only_when_geometry_is_asked_for() {
    let dom = parse(PAGE);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    // nobody has asked: no boxes, no layout
    assert!(!l.has_boxes());
    assert_eq!(l.runs(), 0);
    assert_eq!(l.counters().laid, 0);
    // asking for one element's geometry lays the document out once
    let d = rect(&mut l, &dom, "d");
    assert_eq!(d[2], 632.0);
    assert!(l.has_boxes());
    assert_eq!(l.runs(), 1);
    rect(&mut l, &dom, "p");
    l.fragments();
    assert_eq!(l.runs(), 1);
}

#[test]
fn unchanged_boxes_reuse_their_last_fragments() {
    let dom = parse(PAGE);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    let before = l.fragments().expect("laid out").frag.clone();
    let laid = l.counters().laid;
    let d = by_id(&dom, "d");
    l.invalidate(d);
    let after = l.fragments().expect("laid out").frag.clone();
    assert_eq!(l.runs(), 2);
    // only #d's box and its ancestors were laid out again; #p's fragment is the very same one
    let c = l.counters();
    assert!(c.laid - laid <= 4, "{c:?}");
    assert!(c.reused >= 1, "{c:?}");
    let p_of = |f: &std::sync::Arc<madcawl_layout::Fragment>| {
        let mut found = None;
        f.walk(|_, x| {
            if x.node == Some(by_id(&dom, "p")) && found.is_none() {
                found = Some(x as *const _);
            }
        });
        found
    };
    let (a, b) = (p_of(&before), p_of(&after));
    assert!(a.is_some() && a == b);
}

// a chain of `depth` elements under body, with text at the bottom
fn deep(depth: usize) -> madcawl_dom::Dom {
    deep_of(depth, None)
}

fn deep_of(depth: usize, style: Option<&[u8]>) -> madcawl_dom::Dom {
    let mut dom = parse("<!doctype html><body>");
    let body = dom
        .descendants(dom.document())
        .find(|&n| dom.local_name(n) == Some(madcawl_str::atom!("body")))
        .expect("a body");
    let mut at: NodeId = body;
    for _ in 0..depth {
        let el = dom.new_element(b"div").expect("room");
        if let Some(v) = style {
            dom.set_attribute(el, madcawl_str::atom!("style"), v);
        }
        dom.insert_node(at, el, None);
        at = el;
    }
    let t = dom.new_text(DomString::from_utf16(&[0x78])).expect("room");
    dom.insert_node(at, t, None);
    dom
}

#[test]
fn a_tree_deeper_than_the_cap_lays_out_without_overflowing() {
    let dom = deep(20_000);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    let root = l.fragments().expect("laid out").frag.clone();
    // the text below the cap still makes a line
    assert!(root.size.h > Au::ZERO);
    assert!(
        l.boxes().depth() <= 2 * MAX_DEPTH + 2,
        "{}",
        l.boxes().depth()
    );
}

#[test]
fn deep_inline_blocks_and_floats_stay_within_the_stack() {
    for v in [
        &b"display:inline-block"[..],
        b"float:left",
        b"position:absolute",
    ] {
        let dom = deep_of(5_000, Some(v));
        let st = style(&dom);
        let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
        assert!(l.fragments().is_some());
    }
}

#[test]
fn huge_lengths_saturate() {
    let dom = parse(
        "<!doctype html><body style='margin:0'><div id=a style='height:1e30px;margin-top:1e30px'></div>\
         <div id=b style='height:1e30px'></div><div id=c style='width:-1e30px;margin-left:1e30px'></div>",
    );
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    let root = l.fragments().expect("laid out").frag.clone();
    assert_eq!(root.size.h, Au::MAX);
    assert!(l.border_box(by_id(&dom, "c")).is_some());
}
