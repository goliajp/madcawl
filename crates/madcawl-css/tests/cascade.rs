//! The cascade's order: origins and importance, the style attribute,
//! layers, specificity, order of appearance, and the CSS-wide keywords.

#[allow(dead_code)]
mod style;

use madcawl_css::style::{ContentVisibility, Device, Display, Visibility};
use style::{page, page_on};

#[test]
fn the_user_agent_sheet_decides_what_is_displayed() {
    let p = page(
        "<!doctype html><title id=t>x</title><div id=d><span id=s>a</span></div>\
         <ul><li id=li>x</li></ul><table><tr id=tr><td id=td>x</td></tr></table>\
         <input id=i><input id=h type=HIDDEN><script id=sc></script><template id=tp></template>\
         <dialog id=dg></dialog><dialog id=dgo open></dialog><audio id=au></audio>",
    );
    for (id, d) in [
        ("t", Display::None),
        ("d", Display::Block),
        ("s", Display::Inline),
        ("li", Display::ListItem),
        ("tr", Display::TableRow),
        ("td", Display::TableCell),
        ("i", Display::InlineBlock),
        ("h", Display::None),
        ("sc", Display::None),
        ("tp", Display::None),
        ("dg", Display::None),
        ("dgo", Display::Block),
        ("au", Display::None),
    ] {
        assert_eq!(p.display(id), d, "#{id}");
    }
    let html = p
        .dom
        .descendants(p.dom.document())
        .find(|&n| p.styles.get(n).is_some());
    assert_eq!(p.styles.get(html.unwrap()).unwrap().display, Display::Block);
}

#[test]
fn hidden_and_until_found() {
    let p = page("<div id=a hidden><p id=b>x</div><div id=c hidden=until-found><p id=d>y</div>");
    assert_eq!(p.display("a"), Display::None);
    assert!(!p.rendered("a"));
    assert!(!p.rendered("b"), "below display: none");
    assert_eq!(p.display("b"), Display::Block, "its own value stays");
    assert_eq!(p.style("c").content_visibility, ContentVisibility::Hidden);
    assert!(p.rendered("c"));
    assert!(!p.rendered("d"), "its contents are skipped");
    assert!(!p.styles.is_rendered(&p.dom, p.text_in("c")));
}

#[test]
fn specificity_then_order() {
    let p = page(
        "<style>#a { display: block } div.x { display: none } .y { display: flex } \
         .z { display: grid }</style><div id=a class=x></div><span id=b class='y z'></span>",
    );
    assert_eq!(p.display("a"), Display::Block);
    assert_eq!(p.display("b"), Display::Grid);
}

#[test]
fn importance_and_the_style_attribute() {
    let p = page(
        "<style>.x { display: none !important } #a { display: block } #b { display: block } \
         #c { display: flex !important } input { display: block !important }</style>\
         <div id=a class=x></div><div id=b style='display: inline'></div>\
         <div id=c style='display: grid'></div><div id=d style='display: grid !important' class=x>\
         </div><input id=e type=hidden>",
    );
    assert_eq!(p.display("a"), Display::None);
    assert_eq!(
        p.display("b"),
        Display::Inline,
        "the style attribute beats an id"
    );
    assert_eq!(
        p.display("c"),
        Display::Flex,
        "an important rule beats a normal attribute"
    );
    assert_eq!(
        p.display("d"),
        Display::Grid,
        "an important attribute beats an important rule"
    );
    assert_eq!(
        p.display("e"),
        Display::None,
        "important user agent rules win"
    );
}

#[test]
fn layers() {
    let p = page(
        "<style>@layer a, b; @layer b { .x { display: none } } @layer a { #p { display: block } }\
         .y { display: inline } @layer a { #q { display: none } }\
         @layer a { .z { display: none !important } } .z { display: block !important }\
         @layer c { @layer d { .w { display: flex } } .w { display: grid } }\
         @layer { .v { display: none } } .v { display: table }</style>\
         <p id=p class=x></p><p id=q class=y></p><p id=r class=z></p><p id=s class=w></p>\
         <p id=t class=v></p>",
    );
    assert_eq!(
        p.display("p"),
        Display::None,
        "a later layer beats specificity"
    );
    assert_eq!(p.display("q"), Display::Inline, "unlayered beats layered");
    assert_eq!(
        p.display("r"),
        Display::None,
        "important layered beats important unlayered"
    );
    assert_eq!(
        p.display("s"),
        Display::Grid,
        "a layer's own rules beat its sublayers'"
    );
    assert_eq!(p.display("t"), Display::Table);
}

#[test]
fn revert_and_revert_layer() {
    let p = page(
        "<style>div { display: inline } #a { display: revert }\
         @layer one { #b { display: flex } } @layer two { #b { display: revert-layer } }\
         span { display: revert }</style><div id=a></div><section id=b></section><span id=c></span>",
    );
    assert_eq!(p.display("a"), Display::Block, "back to the user agent's");
    assert_eq!(p.display("b"), Display::Flex, "back to the layer below");
    assert_eq!(p.display("c"), Display::Inline);
}

#[test]
fn wide_keywords_and_all() {
    let p = page(
        "<style>#a { display: initial } #b { all: unset } #c { visibility: hidden }\
         #d { visibility: initial } #e { display: inherit } #f { display: var(--x) }</style>\
         <div id=a></div><div id=b></div><div id=c><span id=d></span><span id=e></span></div>\
         <div id=f></div>",
    );
    assert_eq!(p.display("a"), Display::Inline);
    assert_eq!(p.display("b"), Display::Inline);
    assert_eq!(p.style("d").visibility, Visibility::Visible);
    assert_eq!(p.display("e"), Display::Block);
    assert_eq!(
        p.display("f"),
        Display::Inline,
        "an unresolved var() acts as unset"
    );
}

#[test]
fn media_and_supports() {
    let html = "<style>@media (max-width: 600px) { #a { display: none } }\
         @media screen and (min-width: 600px) { #b { display: none } }\
         @media print { #c { display: none } } @media not print { #d { display: none } }\
         @media (600px <= width <= 2000px) { #e { display: none } }\
         @media (prefers-color-scheme: dark) { #f { display: none } }\
         @media (unknown-feature) { #g { display: none } }\
         @supports (display: grid) { #h { display: none } }\
         @supports (display: frob) or (not (display: flex)) { #i { display: none } }\
         @supports selector(:has(a)) { #j { display: none } }</style>\
         <style media=print>#k { display: none }</style>\
         <p id=a><p id=b><p id=c><p id=d><p id=e><p id=f><p id=g><p id=h><p id=i><p id=j><p id=k>";
    let p = page(html);
    let hidden: Vec<&str> = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"]
        .into_iter()
        .filter(|id| p.display(id) == Display::None)
        .collect();
    assert_eq!(hidden, ["b", "d", "e", "h", "j"]);
    let narrow = Device {
        width: 400.0,
        dark: true,
        ..Device::default()
    };
    let p = page_on(html, narrow);
    assert_eq!(p.display("a"), Display::None);
    assert_eq!(p.display("b"), Display::Block);
    assert_eq!(p.display("f"), Display::None);
}

#[test]
fn nesting() {
    let p = page(
        "<style>.a { .b { display: none } &.c { display: flex } > .d { display: grid }\
         .e & { display: table } display: block; @media all { display: inline-block } }\
         </style><div class=a id=p><span class=b id=b></span><i class=d id=d></i></div>\
         <div class='a c' id=c></div><div class=e><span class=a id=e></span></div>\
         <span class=b id=nb></span>",
    );
    assert_eq!(p.display("b"), Display::None);
    assert_eq!(p.display("nb"), Display::Inline);
    assert_eq!(p.display("c"), Display::Flex);
    assert_eq!(p.display("d"), Display::Grid);
    assert_eq!(p.display("e"), Display::Table);
    assert_eq!(
        p.display("p"),
        Display::InlineBlock,
        "nested declarations come later"
    );
}

#[test]
fn inheritance_and_visibility() {
    let p = page(
        "<style>#a { visibility: hidden } #c { visibility: visible } #d { opacity: 0.25 }\
         #e { opacity: 150% }</style>\
         <div id=a><p id=b>x</p><p id=c>y</p></div><p id=d></p><p id=e></p>",
    );
    assert!(p.rendered("b"));
    assert!(!p.visible("b"));
    assert!(p.visible("c"));
    assert!(!p.styles.is_visible(&p.dom, p.text_in("b")));
    assert_eq!(p.style("d").opacity(), 0.25);
    assert_eq!(p.style("e").opacity(), 1.0);
    assert_eq!(p.style("b").opacity(), 1.0, "opacity does not inherit");
}

#[test]
fn blockification() {
    let p = page(
        "<style>html { display: inline } #a { float: left } #b { position: absolute; float: right }\
         #f { display: flex } #g { display: contents } #t { display: inline-table; float: left }\
         #s { position: -webkit-sticky }</style>\
         <span id=a></span><span id=b></span><div id=f><span id=c></span><div id=g>\
         <span id=d></span></div></div><span id=t></span><span id=s></span>",
    );
    assert_eq!(p.display("a"), Display::Block);
    assert_eq!(p.display("b"), Display::Block);
    assert_eq!(p.style("b").float, madcawl_css::style::Float::None);
    assert_eq!(p.display("c"), Display::Block, "a flex item");
    assert_eq!(p.display("g"), Display::Contents);
    assert_eq!(
        p.display("d"),
        Display::Block,
        "a flex item through display: contents"
    );
    assert_eq!(p.display("t"), Display::Table);
    assert_eq!(p.style("s").position, madcawl_css::style::Position::Sticky);
}

#[test]
fn what_html_and_svg_never_render() {
    let p = page(
        "<details id=d><summary id=s>open me</summary><p id=p>inside</p>loose</details>\
         <details id=o open><p id=op>shown</p></details><canvas id=c><p id=cp>fallback</p></canvas>\
         <svg id=g><title id=t>name</title><defs><g id=dg></g></defs><text id=x>drawn</text></svg>",
    );
    assert!(p.rendered("d"));
    assert!(p.rendered("s"));
    assert!(!p.rendered("p"));
    let loose = p.dom.last_child(p.el("d")).unwrap();
    assert!(!p.styles.is_rendered(&p.dom, loose));
    assert!(p.styles.is_rendered(&p.dom, p.text_in("s")));
    assert!(p.rendered("op"));
    assert!(p.rendered("c"));
    assert!(!p.rendered("cp"));
    assert!(!p.rendered("t"));
    assert!(!p.rendered("dg"));
    assert!(p.rendered("x"));
    assert_eq!(
        p.display("p"),
        Display::Block,
        "computed values are untouched"
    );
}
