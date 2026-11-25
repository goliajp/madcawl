//! Restyling after a change: a class, id, attribute or state change
//! restyles the range its selectors reach and no more, an appended
//! element restyles what `:last-child` and `:empty` make it, and the
//! values after a restyle are the ones a full run gives.

#[allow(dead_code)]
mod style;

use madcawl_css::selector::PseudoClass;
use madcawl_css::style::{Change, ElementStates};
use madcawl_dom::Dom;
use madcawl_html::{Atom, Attrs, Namespace, QuirksMode, TreeSink, atom};
use style::{Pair, atom_of, by_id, cascade, check, parse, set, sheets};

const SIBLINGS: &str = "<b id=a></b><b id=b></b><b id=c></b><b id=d></b>";

#[test]
fn a_class_only_in_rightmost_compounds_restyles_the_element() {
    let mut dom = parse(&format!(
        "<!doctype html><style>.on {{ display: block }}</style><div id=p>{SIBLINGS}<i id=k><u></u></i></div>"
    ));
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let b = by_id(&dom, "b");
    let ch = [set(&mut dom, b, atom!("class"), "on")];
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert_eq!((r.styled, r.changed, r.full), (1, 1, false));
    // a change its children inherit restyles them too
    let k = by_id(&dom, "k");
    let ch = [set(&mut dom, k, atom!("style"), "color: red")];
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert_eq!((r.styled, r.changed), (2, 2));
}

#[test]
fn left_of_a_sibling_combinator_restyles_the_later_siblings() {
    let mut dom = parse(&format!(
        "<!doctype html><style>.x + b {{ display: block }} .x ~ b {{ float: left }}</style><div>{SIBLINGS}</div>"
    ));
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let b = by_id(&dom, "b");
    let ch = [set(&mut dom, b, atom!("class"), "x")];
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert_eq!(r.styled, 3, "#b, #c and #d");
    assert_eq!(r.changed, 2);
}

#[test]
fn left_of_a_descendant_combinator_restyles_the_subtree() {
    let mut dom = parse(
        "<!doctype html><style>#z i { display: block }</style>\
         <div id=p><i></i><span><i></i></span></div><div id=q><i></i></div>",
    );
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let p = by_id(&dom, "p");
    let ch = [set(&mut dom, p, atom!("id"), "z")];
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert_eq!(r.styled, 4, "#p and its three descendants");
    assert_eq!(r.changed, 2);
}

#[test]
fn attributes_reach_as_their_selectors_say() {
    let mut dom = parse(
        "<!doctype html><style>[data-k] { display: block } [data-t=on] span { float: left }</style>\
         <div id=a><span></span></div><div id=b data-t=off><span></span><span></span></div><p data-k></p>",
    );
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let a = by_id(&dom, "a");
    let ch = [set(&mut dom, a, atom_of("data-k"), "")];
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert_eq!(r.styled, 1, "#a, whose display children do not inherit");
    let b = by_id(&dom, "b");
    let ch = [set(&mut dom, b, atom_of("data-t"), "on")];
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert_eq!(r.styled, 3, "#b and both spans");
    assert_eq!(r.changed, 2);
}

#[test]
fn states_set_by_the_host() {
    let dom = parse(&format!(
        "<!doctype html><style>b:hover {{ display: block }} div:focus b {{ float: left }}</style>\
         <div id=p>{SIBLINGS}</div>"
    ));
    let (list, mut st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let b = by_id(&dom, "c");
    st.set(b, PseudoClass::Hover, true);
    let ch = [Change::State {
        el: b,
        pseudo: PseudoClass::Hover,
    }];
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert_eq!((r.styled, r.changed), (1, 1));
    assert_eq!(s.get(b).unwrap().display.css(), "block");
    let p = by_id(&dom, "p");
    st.set(p, PseudoClass::Focus, true);
    let ch = [Change::State {
        el: p,
        pseudo: PseudoClass::Focus,
    }];
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert_eq!(r.styled, 5, "the div and its four children");
    assert_eq!(r.changed, 4);
}

fn append(dom: &mut Dom, parent: &str, name: Atom) -> Change {
    let p = by_id(dom, parent);
    let el = dom.create_element(Namespace::Html, name, Attrs::empty());
    dom.append(p, el);
    Change::Appended(el)
}

#[test]
fn appending_restyles_what_was_last_and_the_parent_that_was_empty() {
    let mut dom = parse(
        "<!doctype html><style>li:last-child { display: none } ul:empty { float: left }</style>\
         <ul id=u><li></li><li></li></ul><ul id=e></ul>",
    );
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let ch = [append(&mut dom, "u", atom!("li"))];
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert_eq!(r.styled, 2, "the new item and the one before it");
    let ch = [append(&mut dom, "e", atom!("li"))];
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert_eq!(r.styled, 2, "the new item and its parent");
}

#[test]
fn appending_where_nothing_asks_what_is_last_styles_only_the_new_element() {
    let mut dom =
        parse("<!doctype html><style>li { color: red }</style><ul id=u><li></li><li></li></ul>");
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let ch = [append(&mut dom, "u", atom!("li"))];
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert_eq!((r.styled, r.full), (1, false));
    assert_eq!(s.stats().hits, 1, "the new item has its siblings' key");
}

#[test]
fn has_restyles_the_document() {
    let mut dom =
        parse("<!doctype html><style>div:has(.x) { display: none }</style><div><b id=b></b></div>");
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let b = by_id(&dom, "b");
    let ch = [set(&mut dom, b, atom!("class"), "x")];
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert!(r.full);
}

#[test]
fn styles_of_another_cascade_are_made_again() {
    let dom = parse("<!doctype html><style>p { color: red }</style><p id=p>x</p>");
    let (list, st) = (sheets(&dom), ElementStates::new());
    let mut s = cascade(&list).compute_with(&dom, &st);
    let c = Pair::new(&list);
    let r = check(&c, &dom, &mut s, &st, &[]);
    assert!(r.full);
}

#[test]
fn a_sibling_combinator_inside_is_or_not_left_of_a_descendant_reaches_subtrees() {
    for (css, html) in [
        (
            ":is(.a + .b) .c { color: red }",
            "<p id=x class=z></p><p class=b><span class=c>x</span></p>",
        ),
        (
            ":not(.a + *) > .c { color: red }",
            "<p id=x class=z></p><p><span class=c>x</span></p>",
        ),
    ] {
        let mut dom = parse(&format!(
            "<!doctype html><style>{css}</style><div>{html}</div>"
        ));
        let (list, st) = (sheets(&dom), ElementStates::new());
        let c = Pair::new(&list);
        let mut s = c.compute_with(&dom, &st);
        let x = by_id(&dom, "x");
        let ch = [set(&mut dom, x, atom!("class"), "a")];
        let r = check(&c, &dom, &mut s, &st, &ch);
        assert_eq!(r.styled, 3, "{css}: #x, the next p and its span");
    }
}

#[test]
fn styles_made_in_another_quirks_mode_are_made_again() {
    let mut dom = parse("<style>.K { color: red }</style><p class=k id=p>x</p>");
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    dom.set_quirks_mode(QuirksMode::NoQuirks);
    let r = check(&c, &dom, &mut s, &st, &[]);
    assert!(r.full);
}

#[test]
fn replacing_a_large_style_attribute_keeps_path_key_bytes_bounded() {
    let mut dom = parse("<!doctype html><div id=a><i></i></div><p></p><p></p>");
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let a = by_id(&dom, "a");
    let pad = "x".repeat(256 * 1024);
    let mut fulls = 0;
    for i in 0..40 {
        let ch = [set(&mut dom, a, atom!("style"), &format!("--k{i}: {pad}"))];
        let r = c.keyed.restyle(&dom, &mut s, &st, &ch);
        fulls += usize::from(r.full);
        let bound = 2 * dom.bytes() + (1 << 20) + 2 * pad.len();
        assert!(
            s.path_key_bytes() <= bound,
            "step {i}: {} > {bound}",
            s.path_key_bytes()
        );
    }
    assert!(fulls > 0);
    assert!(s.path_keys() < 64);
}

#[test]
fn an_after_appearing_counts_the_element_as_changed() {
    let mut dom = parse(
        "<!doctype html><style>.on::after { content: 'x' }</style><div id=p><b id=a><i></i></b></div>",
    );
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let a = by_id(&dom, "a");
    let ch = [set(&mut dom, a, atom!("class"), "on")];
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert_eq!((r.styled, r.changed, r.full), (1, 1, false));
}
