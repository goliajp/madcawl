//! The buckets only leave untried rules that cannot match, `:is()` and `:where()` lists included:
//! every element's values and pseudo-element values are the same as when every rule is tried against
//! every element.

#[allow(dead_code)]
mod style;

use madcawl_css::style::Styles;
use madcawl_dom::{Dom, NodeKind};
use style::{cascade, differences, parse, sheets};

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

#[track_caller]
fn same_with_every_rule(html: &str) {
    let dom = parse(html);
    let list = sheets(&dom);
    let mut on = cascade(&list);
    on.set_verify(true);
    let mut off = cascade(&list);
    off.set_buckets(false);
    off.set_path_key(false);
    let (a, b) = (on.compute(&dom), off.compute(&dom));
    assert_eq!(
        differences(&dom, &a, &b),
        0,
        "values differ with the buckets"
    );
    assert_eq!(
        pseudo_differences(&dom, &a, &b),
        0,
        "pseudo-elements differ with the buckets"
    );
    assert_eq!(a.stats().mismatches, 0);
    // the rules under test changed something
    let plain = cascade(&[]).compute(&dom);
    assert!(differences(&dom, &a, &plain) > 0);
}

const SHEET: &str = r#"<style>
    @namespace xlink url(http://www.w3.org/1999/xlink);
    :root { --c: rgb(1 2 3); color: var(--c) }
    :root[data-mode=dark] { color: rgb(4 5 6) }
    [data-mode=dark] p { margin-left: 3px }
    [data-x] { padding-left: 1px }
    [data-x="b" i] { padding-left: 2px }
    [DATA-Y] { padding-top: 5px }
    [viewBox] { margin-top: 7px }
    [href] { font-weight: 700 }
    [href]::before { content: "→" }
    [title]::after { content: attr(title); display: block }
    [hidden] { display: none }
    :is([lang], [dir]) { margin-right: 4px }
    *:root { line-height: 2 }
    .a.a { border-top: 1px solid }
    :where(.p, .Q) { padding-bottom: 6px }
    :is(#i, span, [data-z], div .b) > i { margin-bottom: 8px }
    :is(.r, :first-child) { text-indent: 9px }
    :where(.p, .q)::before { content: "q" }
    .A { border-bottom: 2px solid }
    [xlink|href] { margin-left: 11px }
    [*|href] { padding-right: 12px }
    :not([data-x]) { letter-spacing: 1px }
    #x::before { content: "x" }
    #x { word-spacing: 2px }
    [viewbox] { padding-top: 13px }
    [VIEWBOX] { padding-bottom: 14px }
    [viewBox][preserveAspectRatio] { margin-right: 15px }
</style>"#;

#[test]
fn root_and_attribute_rules_match_as_every_rule_tried() {
    let body = r##"
        <p data-x>a</p><p data-x="B">b</p><p data-y="1" data-x="b">c</p>
        <svg viewBox="0 0 1 1"><a href="#x"><text data-x>d</text></a></svg>
        <math><mi href="#y">e</mi></math>
        <a href="/z" title="t">f</a><span hidden>g</span>
        <div lang=fr><p dir=rtl class="a a A">h</p></div>
        <p class="p q">i</p><p class=q>j</p><b id=I><i>k</i></b><span><i>l</i></span>
        <em data-z><i>m</i></em><div><p class=b><i>n</i></p></div><p class=r>o</p>
        <p id=x>p</p><svg viewBox="0 0 2 2" preserveAspectRatio=none><a xlink:href="#q">r</a></svg>"##;
    same_with_every_rule(&format!(
        r#"<!doctype html><html data-mode=dark>{SHEET}{body}"#
    ));
    // quirks mode: classes and ids match case-insensitively
    same_with_every_rule(&format!(r#"<html data-mode=dark>{SHEET}{body}"#));
}

#[test]
fn an_element_with_one_attribute_name_twice_styles_as_every_rule_tried() {
    let html = format!(
        r##"<!doctype html>{SHEET}<svg><a xlink:href="#a" href="#b" xml:lang=en lang=fr>x</a></svg>"##
    );
    same_with_every_rule(&html);
}
