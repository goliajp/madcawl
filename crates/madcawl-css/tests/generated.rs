//! `content`, `aspect-ratio`, and the computed values of `::before` and `::after`.

#[allow(dead_code)]
mod style;

use madcawl_css::style::{Content, ContentItem, Display, Float, Prop};
use style::page;

#[test]
fn content_and_aspect_ratio_compute_and_serialize() {
    let p = page(
        "<!doctype html><style>#a { content: 'x' attr(title) open-quote counter(n) } #b { content: none } \
         #c { aspect-ratio: 16 / 9 } #d { aspect-ratio: auto 2 } #e { aspect-ratio: 0 / 1 } \
         #f { aspect-ratio: 1 / 2 auto }</style>\
         <div id=a></div><div id=b></div><div id=c></div><div id=d></div><div id=e></div><div id=f></div>\
         <div id=g></div><img id=h width=200 height=100><img id=i width=200>",
    );
    let css = |id: &str, prop: &str| p.style(id).css(Prop::from_name(prop).expect("a property"));
    assert_eq!(
        css("a", "content"),
        "\"x\" attr(title) open-quote counter(n)"
    );
    assert_eq!(css("b", "content"), "none");
    assert_eq!(css("g", "content"), "normal");
    assert_eq!(css("c", "aspect-ratio"), "16 / 9");
    assert_eq!(css("d", "aspect-ratio"), "auto 2 / 1");
    assert_eq!(css("e", "aspect-ratio"), "auto");
    assert_eq!(css("f", "aspect-ratio"), "auto 1 / 2");
    assert_eq!(css("g", "aspect-ratio"), "auto");
    // an image's width and height attributes give it a ratio, both or neither
    assert_eq!(css("h", "aspect-ratio"), "auto 200 / 100");
    assert_eq!(css("i", "aspect-ratio"), "auto");
}

#[test]
fn before_and_after_cascade_from_their_elements_rules_and_values() {
    let p = page(
        "<!doctype html><style>.x::before { content: 'b'; color: red } div:after { content: 'a' } \
         #n::after { content: none } #f { display: flex } #f::before { content: ''; float: left } \
         p::before { display: none; content: 'p' } #h:hover::before { content: 'h' }</style>\
         <div id=a class=x style='color: blue; font-size: 30px'></div><div id=n></div><div id=f></div>\
         <p id=p></p><span id=h></span>",
    );
    let a = p.el("a");
    let before = p.styles.pseudo(a, false).expect("a ::before");
    let after = p.styles.pseudo(a, true).expect("an ::after");
    assert_eq!(
        before.rare.content,
        Content::Items([ContentItem::Text("b".into())].into())
    );
    // the element's inline style is its own, inherited by its pseudo-elements, not theirs
    assert_eq!(before.font.size(), 30.0);
    assert_ne!(before.text.color, after.text.color);
    assert_eq!(
        after.rare.content,
        Content::Items([ContentItem::Text("a".into())].into())
    );
    assert!(p.styles.pseudo(p.el("n"), true).is_none());
    // a flex container's pseudo-elements are its items: blockified, never floating
    let f = p.styles.pseudo(p.el("f"), false).expect("a ::before");
    assert_eq!((f.display, f.float), (Display::Block, Float::None));
    assert!(p.styles.pseudo(p.el("p"), false).is_none());
    assert!(p.styles.pseudo(p.el("h"), false).is_none());
}

#[test]
fn elements_alike_but_for_a_pseudo_element_rule_that_needs_more_get_their_own_pseudo_elements() {
    let p = page(
        "<!doctype html><style>li:nth-child(2)::before { content: 'two' } li.k::after { content: 'k' } \
         li:nth-child(5)::after { color: red } *, ::before, ::after { box-sizing: border-box }</style>\
         <ul><li id=a></li><li id=b></li><li id=c></li><li id=d class=k></li><li id=e class=k></li></ul>",
    );
    let has = |id: &str, after: bool| p.styles.pseudo(p.el(id), after).is_some();
    assert_eq!(
        ["a", "b", "c"].map(|id| has(id, false)),
        [false, true, false]
    );
    assert!(has("d", true) && has("e", true) && !has("c", true));
    let (d, e) = (p.styles.pseudo(p.el("d"), true), p.styles.pseudo(p.el("e"), true));
    assert_ne!(d.map(|s| &s.text.color), e.map(|s| &s.text.color));
    assert_eq!(
        e.map(|s| s.boxm.box_sizing),
        Some(madcawl_css::style::BoxSizing::BorderBox)
    );
}

#[test]
fn a_rule_that_needs_more_hiding_one_elements_pseudo_element_leaves_the_next_ones() {
    let p = page(
        "<!doctype html><style>li::after { content: 'x' } li:nth-child(1)::after { display: none }</style>\
         <ul><li id=a></li><li id=b></li><li id=c></li></ul>",
    );
    let has = |id: &str| p.styles.pseudo(p.el(id), true).is_some();
    assert_eq!(["a", "b", "c"].map(has), [false, true, true]);
}
