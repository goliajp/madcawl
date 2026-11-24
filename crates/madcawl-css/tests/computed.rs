//! The computed values layout reads: lengths in 1/64 px, fonts, colors,
//! the box, flex and the shorthands that set them, and the groups
//! children share.

#[allow(dead_code)]
mod style;

use madcawl_css::style::{Prop, Size};
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
fn lengths_resolve_against_the_font_and_the_viewport() {
    let p = page(
        "<!doctype html><style>html { font-size: 10px } #a { font-size: 20px; margin: 1em 2rem 50% 1.5px; \
         padding-left: calc(10% + 1em); width: 10vw; height: calc(100vh - 2 * 100px); text-indent: -1em } \
         #b { font-size: 150%; margin-top: 1em; min-width: max(1em, 40px); max-width: min(10%, 5px) }</style>\
         <div id=a><span id=b></span></div>",
    );
    check(
        &p,
        &[
            ("a", "margin-top", "20px"),
            ("a", "margin-right", "20px"),
            ("a", "margin-bottom", "50%"),
            ("a", "margin-left", "1.5px"),
            ("a", "padding-left", "calc(10% + 20px)"),
            ("a", "width", "128px"),
            ("a", "height", "600px"),
            ("a", "text-indent", "-20px"),
            ("b", "font-size", "30px"),
            ("b", "margin-top", "30px"),
            ("b", "min-width", "40px"),
            ("b", "max-width", "none"),
        ],
    );
    let b = p.style("a");
    let Size::Lp(m) = b.boxm.margin[3] else {
        panic!("a length")
    };
    assert_eq!(m.px, 96, "1.5px in 1/64 px");
}

#[test]
fn fonts() {
    let p = page(
        "<!doctype html><style>#a { font: italic bold 12px/1.5 \"Helvetica Neue\", Arial, sans-serif } \
         #c { font-weight: 300 } #c b { font-weight: bolder } #d { font-size: larger } \
         #e { line-height: 150% } #f { font-family: monospace }</style>\
         <div id=a><span id=a1>x</span></div><div id=c><b id=c1>x</b></div><p id=d>x</p>\
         <p id=e>x</p><p id=f>x</p><pre id=g>x</pre><pre id=h style='font-family: serif'>x</pre><h1 id=h1>x</h1>",
    );
    check(
        &p,
        &[
            ("a", "font-style", "italic"),
            ("a", "font-weight", "700"),
            ("a", "font-size", "12px"),
            ("a", "line-height", "18px"),
            ("a", "font-family", "\"Helvetica Neue\", Arial, sans-serif"),
            ("a1", "line-height", "18px"),
            ("c1", "font-weight", "400"),
            ("d", "font-size", "18px"),
            ("e", "line-height", "24px"),
            ("f", "font-size", "13px"),
            ("g", "font-size", "13px"),
            ("g", "white-space-collapse", "preserve"),
            ("h", "font-size", "16px"),
            ("h1", "font-size", "32px"),
            ("h1", "margin-top", "21.4375px"),
            ("h1", "font-weight", "700"),
        ],
    );
}

#[test]
fn colors_and_currentcolor() {
    let p = page(
        "<!doctype html><style>#a { color: #f00; border: 1px solid; background: url(x.png) no-repeat #00ff0080 } \
         #b { color: currentcolor; border-top-color: rgb(1 2 3 / 50%) } #c { background-color: transparent } \
         #d { background: none } #e { color: oklch(0.5 0.1 200) }</style>\
         <div id=a><div id=b></div></div><div id=c></div><div id=d></div><a id=l href=x>x</a><p id=e></p>",
    );
    check(
        &p,
        &[
            ("a", "color", "rgb(255, 0, 0)"),
            ("a", "border-left-color", "rgb(255, 0, 0)"),
            ("a", "background-color", "rgba(0, 255, 0, 0.5)"),
            ("b", "color", "rgb(255, 0, 0)"),
            ("b", "border-top-color", "rgba(1, 2, 3, 0.5)"),
            ("c", "background-color", "rgba(0, 0, 0, 0)"),
            ("d", "background-color", "rgba(0, 0, 0, 0)"),
            ("l", "color", "rgb(0, 0, 238)"),
            ("e", "color", "oklch(0.5 0.1 200)"),
        ],
    );
}

#[test]
fn borders_snap_and_vanish_without_a_style() {
    let p = page(
        "<!doctype html><style>#a { border-width: 0.5px 2.7px thick thin; border-style: solid } \
         #b { border: 4px none red } #c { border-top: medium dashed }</style>\
         <div id=a></div><div id=b></div><div id=c></div>",
    );
    check(
        &p,
        &[
            ("a", "border-top-width", "1px"),
            ("a", "border-right-width", "2px"),
            ("a", "border-bottom-width", "5px"),
            ("a", "border-left-width", "1px"),
            ("b", "border-top-width", "0px"),
            ("b", "border-top-style", "none"),
            ("c", "border-top-width", "3px"),
            ("c", "border-top-style", "dashed"),
            ("c", "border-left-width", "0px"),
        ],
    );
}

#[test]
fn overflow_on_one_axis_makes_the_other_scroll() {
    let p = page(
        "<!doctype html><style>#a { overflow-x: hidden } #b { overflow: clip visible } \
         #c { overflow: scroll clip }</style><div id=a></div><div id=b></div><div id=c></div>",
    );
    check(
        &p,
        &[
            ("a", "overflow-x", "hidden"),
            ("a", "overflow-y", "auto"),
            ("b", "overflow-x", "clip"),
            ("b", "overflow-y", "visible"),
            ("c", "overflow-y", "hidden"),
        ],
    );
}

#[test]
fn flex_and_alignment() {
    let p = page(
        "<!doctype html><style>#a { display: flex; flex-flow: column wrap; justify-content: space-between; \
         align-items: center; place-content: center } #b { flex: 1 } #c { flex: none; order: -2 } \
         #d { flex: 2 3 10em; align-self: last baseline } #e { flex: auto; z-index: 5 }</style>\
         <div id=a><span id=b></span><span id=c></span><span id=d></span><span id=e></span></div>",
    );
    check(
        &p,
        &[
            ("a", "flex-direction", "column"),
            ("a", "flex-wrap", "wrap"),
            ("a", "justify-content", "center"),
            ("a", "align-content", "center"),
            ("a", "align-items", "center"),
            ("b", "flex-grow", "1"),
            ("b", "flex-shrink", "1"),
            ("b", "flex-basis", "0%"),
            ("b", "display", "block"),
            ("c", "flex-grow", "0"),
            ("c", "flex-basis", "auto"),
            ("c", "order", "-2"),
            ("d", "flex-basis", "160px"),
            ("d", "flex-shrink", "3"),
            ("d", "align-self", "last baseline"),
            ("e", "z-index", "5"),
            ("e", "flex-grow", "1"),
        ],
    );
}

#[test]
fn the_user_agent_box_model() {
    let p = page(
        "<!doctype html><body id=body><p id=p>x</p><ul id=ul><li id=li>x<ul id=ul2></ul></li></ul>\
         <blockquote id=q>x</blockquote><table><tr><td id=td>x</td></tr></table></body>",
    );
    check(
        &p,
        &[
            ("body", "margin-top", "8px"),
            ("body", "margin-left", "8px"),
            ("p", "margin-top", "16px"),
            ("p", "margin-left", "0px"),
            ("ul", "padding-left", "40px"),
            ("ul", "list-style-type", "disc"),
            ("ul2", "list-style-type", "circle"),
            ("ul2", "margin-top", "0px"),
            ("q", "margin-right", "40px"),
            ("td", "padding-top", "1px"),
            ("td", "vertical-align", "middle"),
        ],
    );
}

#[test]
fn var_in_a_shorthand_is_substituted_before_it_is_split() {
    let p = page(
        "<!doctype html><style>:root { --m: 1px 2px; --b: 3px dotted; --bad: red } \
         #a { margin: var(--m) } #b { border-top: var(--b) blue } #c { margin: 7px; margin: var(--bad) }</style>\
         <div id=a></div><div id=b></div><div id=c></div>",
    );
    check(
        &p,
        &[
            ("a", "margin-top", "1px"),
            ("a", "margin-right", "2px"),
            ("a", "margin-bottom", "1px"),
            ("b", "border-top-width", "3px"),
            ("b", "border-top-style", "dotted"),
            ("b", "border-top-color", "rgb(0, 0, 255)"),
            ("c", "margin-top", "0px"),
        ],
    );
}

#[test]
fn invalid_values_leave_the_cascade() {
    let p = page(
        "<!doctype html><style>#a { padding: 5px; padding: -1px; width: 10px; width: red; \
         margin: 1px 2px 3px 4px 5px; font-weight: 1001; z-index: 1.5 }</style><div id=a></div>",
    );
    check(
        &p,
        &[
            ("a", "padding-top", "5px"),
            ("a", "width", "10px"),
            ("a", "margin-top", "0px"),
            ("a", "font-weight", "400"),
            ("a", "z-index", "auto"),
        ],
    );
}

#[test]
fn groups_are_shared_until_written() {
    let p = page(
        "<!doctype html><div id=a style='color: red'><span id=b>x</span><span id=c style='margin: 1px'>y</span>\
         <span id=d style='font-size: 20px'>z</span></div><div id=e></div>",
    );
    let (a, b, c, d, e) = (
        p.style("a"),
        p.style("b"),
        p.style("c"),
        p.style("d"),
        p.style("e"),
    );
    // font, text, box, paint, place, rare
    assert_eq!(
        b.shares_groups(a)[..2],
        [true, true],
        "inherited from the parent"
    );
    let ce = c.shares_groups(e);
    assert!(!ce[2], "the box written");
    assert!(ce[3] && ce[4] && ce[5], "the rest initial");
    assert!(!d.shares_groups(a)[0] && d.shares_groups(a)[1]);
}

#[test]
fn a_line_height_past_the_fixed_point_range_saturates() {
    let p = page(
        "<!doctype html><p id=a style='font-size: 100000000px; line-height: calc(33554431px + 100%)'>x</p>\
         <p id=b style='line-height: calc(-33554431px - 100000000%)'>y</p>",
    );
    check(&p, &[("a", "line-height", "33554431.984375px")]);
    assert!(css(&p, "b", "line-height").starts_with('-'));
}
