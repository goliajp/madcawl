//! The flow-relative properties: each sets the physical side the
//! element's writing mode and direction make it, in one cascade with
//! the physical properties.

#[allow(dead_code)]
mod style;

use madcawl_css::style::Prop;
use style::page;

fn css(p: &style::Page, id: &str, prop: &str) -> String {
    p.style(id).css(Prop::from_name(prop).expect("a property"))
}

#[track_caller]
fn check(p: &style::Page, cases: &[(&str, &str, &str)]) {
    for (id, prop, want) in cases {
        assert_eq!(css(p, id, prop), *want, "#{id} {prop}");
    }
}

#[test]
fn flow_relative_properties_follow_writing_mode_and_direction() {
    let p = page(
        "<!doctype html><style>.x { margin-inline-start: 5px; padding-block: 1px 2px; inset-inline-end: 3px; \
         inline-size: 50px; border-inline-start: 2px solid } #r { direction: rtl } \
         #v { writing-mode: vertical-rl } #o { margin-left: 9px; margin-inline-start: 4px } \
         #o2 { margin-inline-start: 4px; margin-left: 9px }</style>\
         <div id=l class=x></div><div id=r class=x></div><div id=v class=x></div>\
         <div id=o></div><div id=o2></div><div dir=rtl><p id=d class=x></p></div>",
    );
    check(
        &p,
        &[
            ("l", "margin-left", "5px"),
            ("l", "padding-top", "1px"),
            ("l", "padding-bottom", "2px"),
            ("l", "right", "3px"),
            ("l", "width", "50px"),
            ("l", "border-left-width", "2px"),
            ("r", "margin-right", "5px"),
            ("r", "margin-left", "0px"),
            ("r", "left", "3px"),
            ("r", "border-right-style", "solid"),
            ("v", "margin-top", "5px"),
            ("v", "padding-right", "1px"),
            ("v", "padding-left", "2px"),
            ("v", "height", "50px"),
            ("o", "margin-left", "4px"),
            ("o2", "margin-left", "9px"),
            ("d", "direction", "rtl"),
            ("d", "margin-right", "5px"),
        ],
    );
}
