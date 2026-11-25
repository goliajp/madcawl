//! Scroll containers record their scrollable overflow; borders are whole device pixels at the scale the
//! document is laid out at.

mod common;

use common::*;
use madcawl_css::style::{Device, style_document};
use madcawl_layout::{FragKind, Layout};

#[test]
fn a_scroll_container_holds_its_overflow_and_keeps_it_from_its_ancestors() {
    let html = "<!doctype html><body style='margin:0'><div id=o style='height:300px'>\
                <div id=s style='overflow:auto;width:100px;height:50px;border:2px solid'>\
                <div style='width:400px;height:200px'></div></div>\
                <div style='height:10px;margin-left:-20px'></div></div>";
    let dom = parse(html);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    let (s, o) = (by_id(&dom, "s"), by_id(&dom, "o"));
    let root = l.fragments().expect("laid out").frag.clone();
    let (mut scroller, mut outer) = (None, None);
    root.walk(|_, f| {
        if f.kind == FragKind::Box && f.node == Some(s) {
            scroller = Some((f.scroller, f.overflow));
        }
        if f.kind == FragKind::Box && f.node == Some(o) {
            outer = Some((f.scroller, f.overflow));
        }
    });
    let (is, area) = scroller.expect("the scroll container");
    assert!(is);
    assert_eq!(px(area), [0.0, 0.0, 402.0, 202.0]);
    let (is, area) = outer.expect("its parent");
    assert!(!is);
    // the scroll container's overflow stops at its border box; the box beside it overflows to the left
    assert_eq!(px(area), [-20.0, 0.0, 1300.0, 300.0]);
}

#[test]
fn borders_are_whole_device_pixels_at_the_layout_scale() {
    let html = "<!doctype html><body style='margin:0'><div id=a style='width:100px;border:0.5px solid'>\
                </div>";
    let dom = parse(html);
    // computed at 2x, where half a px is a device pixel
    let st = style_document(
        &dom,
        Device {
            scale: 2.0,
            ..Device::default()
        },
        &mut |_| None,
    );
    let mut at2 = Layout::new(&dom, &st, 1280.0, 800.0).with_scale(2.0);
    assert_eq!(rect(&mut at2, &dom, "a"), [0.0, 0.0, 101.0, 1.0]);
    let mut at1 = Layout::new(&dom, &st, 1280.0, 800.0);
    assert_eq!(rect(&mut at1, &dom, "a"), [0.0, 0.0, 102.0, 2.0]);
}
