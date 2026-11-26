//! The ancestor filter only leaves untried rules that cannot match: every element's values, flags and
//! pseudo-element values are the same with it on as with every candidate rule matched, in full runs and in
//! restyles that start below the root.

#[allow(dead_code)]
mod style;

use madcawl_css::style::{Display, ElementStates, StyleStats, Styles};
use madcawl_dom::{Dom, NodeKind};
use madcawl_html::atom;
use style::{by_id, cascade, differences, parse, set, sheets};

fn pseudo_differences(dom: &Dom, a: &Styles, b: &Styles) -> usize {
    dom.descendants(dom.document())
        .filter(|&n| dom.kind(n) == NodeKind::Element)
        .filter(|&n| {
            [false, true]
                .iter()
                .any(|&after| a.pseudo(n, after) != b.pseudo(n, after))
        })
        .count()
}

/// Styles `html` with the filter on and off; the styles with it on and their counts.
#[track_caller]
fn same_with_filter(html: &str) -> (Dom, Styles, StyleStats) {
    let dom = parse(html);
    let list = sheets(&dom);
    let mut on = cascade(&list);
    on.set_verify(true);
    let mut off = cascade(&list);
    off.set_ancestor_filter(false);
    let (a, b) = (on.compute(&dom), off.compute(&dom));
    assert_eq!(
        differences(&dom, &a, &b),
        0,
        "values differ with the filter"
    );
    assert_eq!(
        pseudo_differences(&dom, &a, &b),
        0,
        "pseudo-elements differ with the filter"
    );
    assert_eq!(a.stats().mismatches, 0);
    assert_eq!(b.stats().filter_checked, 0);
    let st = a.stats();
    assert!(
        st.filter_skipped > 0,
        "the filter ruled nothing out: {st:?}"
    );
    (dom, a, st)
}

fn display(dom: &Dom, s: &Styles, id: &str) -> Display {
    s.get(by_id(dom, id)).expect("styled").display
}

const OTHERS: &str = "<p><i>x</i><i>y</i></p><ul><li>a</li><li>b</li></ul>";

#[test]
fn a_sibling_combinator_left_of_a_descendant_one_needs_no_sibling_on_the_chain() {
    let (dom, s, _) = same_with_filter(&format!(
        "<!doctype html><style>.a + .b .c {{ display: block }} .a ~ .b > .c {{ float: left }}</style>\
         <div class=b><span class=c id=u></span></div>\
         <div class=a></div><div class=b><span class=c id=t></span></div>\
         <em></em><div class=b><span class=c id=v></span></div><b class=c id=w></b>{OTHERS}"
    ));
    assert_eq!(display(&dom, &s, "w"), Display::Inline);
    assert_eq!(display(&dom, &s, "t"), Display::Block);
    assert_eq!(display(&dom, &s, "u"), Display::Inline);
    assert_eq!(
        display(&dom, &s, "v"),
        Display::Block,
        "a float is blockified"
    );
}

#[test]
fn quirks_mode_classes_and_ids_match_ancestors_whatever_their_case() {
    let (dom, s, _) = same_with_filter(&format!(
        "<style>.Foo span {{ display: block }} #Bar b {{ display: block }}</style>\
         <div class=foo><span id=t></span></div><div id=bar><b id=u></b></div>\
         <div class=fo><span id=v></span></div>{OTHERS}"
    ));
    assert_eq!(display(&dom, &s, "t"), Display::Block);
    assert_eq!(display(&dom, &s, "u"), Display::Block);
    assert_eq!(display(&dom, &s, "v"), Display::Inline);
}

#[test]
fn foreign_object_type_selectors_keep_their_case() {
    let (dom, s, _) = same_with_filter(&format!(
        "<!doctype html><style>foreignObject span {{ display: block }} svg b {{ display: block }} \
         foreignobject i {{ display: block }}</style>\
         <svg><foreignObject><span id=t></span><b id=u></b><i id=v></i></foreignObject></svg>{OTHERS}"
    ));
    assert_eq!(display(&dom, &s, "t"), Display::Block);
    assert_eq!(display(&dom, &s, "u"), Display::Block);
    assert_eq!(display(&dom, &s, "v"), Display::Inline);
}

#[test]
fn nested_rules_need_what_their_parent_rule_matches() {
    let (dom, s, _) = same_with_filter(&format!(
        "<!doctype html><style>.x {{ .y & {{ display: block }} > .z {{ display: block }} }}</style>\
         <div class=y><b class=x id=t></b></div><b class=x id=u><i class=z id=v></i></b>\
         <b class=x><p><i class=z id=w></i></p></b>{OTHERS}"
    ));
    assert_eq!(display(&dom, &s, "t"), Display::Block);
    assert_eq!(display(&dom, &s, "u"), Display::Inline);
    assert_eq!(display(&dom, &s, "v"), Display::Block);
    assert_eq!(display(&dom, &s, "w"), Display::Inline);
}

#[test]
fn pseudo_elements_need_their_rules_ancestors_too() {
    let (dom, s, _) = same_with_filter(&format!(
        "<!doctype html><style>.a .b::before {{ content: 'x'; display: block }}</style>\
         <div class=a><span class=b id=t></span></div><span class=b id=u></span>{OTHERS}"
    ));
    assert!(s.pseudo(by_id(&dom, "t"), false).is_some());
    assert!(s.pseudo(by_id(&dom, "u"), false).is_none());
}

#[test]
fn a_restyle_from_below_the_root_rebuilds_the_filter_of_its_ancestors() {
    let mut dom = parse(&format!(
        "<!doctype html><style>.on .k {{ display: block }} .on > p > .k {{ float: left }}</style>\
         <div id=r><section id=m><p><span class=k id=t></span></p></section></div>{OTHERS}"
    ));
    let (list, st) = (sheets(&dom), ElementStates::new());
    let on = cascade(&list);
    let mut off = cascade(&list);
    off.set_ancestor_filter(false);
    off.set_path_key(false);
    let mut s = on.compute(&dom);
    assert_eq!(display(&dom, &s, "t"), Display::Inline);
    let m = by_id(&dom, "m");
    for class in ["on", "off", "on"] {
        let ch = [set(&mut dom, m, atom!("class"), class)];
        let r = on.restyle(&dom, &mut s, &st, &ch);
        assert!(!r.full, "an incremental restyle");
        assert!(
            s.stats().filter_checked > 0,
            "the restyle consulted the filter"
        );
        let full = off.compute_with(&dom, &st);
        assert_eq!(differences(&dom, &s, &full), 0, "after class {class}");
        let want = if class == "on" {
            Display::Block
        } else {
            Display::Inline
        };
        assert_eq!(display(&dom, &s, "t"), want);
    }
}

#[test]
fn ancestors_with_many_classes_still_rule_out_rules_naming_others() {
    // pages of utility classes put a hundred names on the root and the body; a filter too small for them
    // is all ones and lets every rule through, each then tried against every ancestor
    let classes = |p: &str| {
        (0..120)
            .map(|i| format!("{p}{i}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let page = |rules: usize| {
        let css: String = (0..rules)
            .map(|k| format!(".absent{k} div div div div span {{ color: red }}"))
            .collect();
        format!(
            "<!doctype html><html class=\"{}\"><style>{css}</style><body class=\"{}\">\
             <div><div><div><div><div><span id=t>x</span></div></div></div></div></div>",
            classes("r"),
            classes("b")
        )
    };
    let skipped = |html: &str| {
        let dom = parse(html);
        let list = sheets(&dom);
        let st = cascade(&list).compute(&dom).stats();
        (st.filter_checked, st.filter_skipped)
    };
    let (none, some) = (skipped(&page(0)), skipped(&page(200)));
    assert_eq!(some.0 - none.0, 200, "each rule asked about once");
    // about 250 names on the ancestors: 256 bits rule out 58 of these, 1024 bits with two a name 174, 4096
    // with three each about 199
    assert!(
        some.1 - none.1 >= 190,
        "only {} of 200 ruled out",
        some.1 - none.1
    );
    same_with_filter(&page(200));
}
