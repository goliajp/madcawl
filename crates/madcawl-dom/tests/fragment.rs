//! The fragment parsing algorithm, as `innerHTML` runs it on a parsed
//! document: the context decides the tokenizer state and the insertion
//! mode, the nodes belong to the context's node document, and the root
//! of the parser's own document is freed.

use madcawl_dom::{Dom, Limits, NodeId, NodeKind};
use madcawl_html::{Options, Step, Tokenizer, TreeBuilder, atom};

fn parse(input: &str) -> Dom {
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(Dom::new(), Options::default());
    tok.feed(input.as_bytes());
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    b.into_sink()
}

fn find(dom: &Dom, name: madcawl_html::Atom) -> NodeId {
    dom.descendants(dom.document()).find(|&n| dom.local_name(n) == Some(name)).unwrap()
}

fn html(dom: &Dom, n: NodeId) -> String {
    let mut out = Vec::new();
    dom.write_html(n, true, &mut out);
    String::from_utf8(out).unwrap()
}

/// `markup` parsed for `context` and put into it, as setting
/// `innerHTML` does: what it serializes to. The fragment and the
/// parser's root are freed, and the counts add up.
fn fragment_of(dom: &mut Dom, context: NodeId, markup: &str) -> String {
    while let Some(c) = dom.first_child(context) {
        dom.remove_node(c);
    }
    let mut freed = Vec::new();
    dom.free_dead(&mut freed);
    let before = dom.node_count();
    let parsed = dom.parse_fragment(context, markup.as_bytes(), true).unwrap();
    assert!(parsed.complete);
    let f = parsed.fragment;
    assert_eq!(dom.kind(f), NodeKind::Fragment);
    assert!(dom.descendants(f).all(|n| dom.owner_document(n) == dom.owner_document(context)));
    let held = dom.hold(f);
    while let Some(c) = dom.first_child(f) {
        dom.insert_node(context, c, None);
    }
    dom.let_go(held);
    dom.free_dead(&mut freed);
    let added = dom.descendants(context).count() as u32;
    assert_eq!(dom.node_count(), before + added, "the fragment and the parser's root are freed");
    assert_eq!(dom.audit().mismatches(), 0, "{:?}", dom.audit());
    html(dom, context)
}

#[test]
fn a_fragment_in_a_div_parses_as_in_body() {
    let mut dom = parse("<div id=c></div>");
    let div = find(&dom, atom!("div"));
    let s = fragment_of(&mut dom, div, "Ray ID: <code>a1</code><p>x<!--c--></p><a href=/x target=_blank>y</a>");
    assert_eq!(s, "Ray ID: <code>a1</code><p>x<!--c--></p><a href=\"/x\" target=\"_blank\">y</a>");
}

#[test]
fn the_context_selects_the_tokenizer_state_and_the_mode() {
    let mut dom = parse("<textarea></textarea><table><tbody></tbody></table>");
    let textarea = find(&dom, atom!("textarea"));
    assert_eq!(fragment_of(&mut dom, textarea, "<b>&amp;</b>"), "&lt;b&gt;&amp;&lt;/b&gt;");
    let tbody = find(&dom, atom!("tbody"));
    assert_eq!(fragment_of(&mut dom, tbody, "<tr><td>1"), "<tr><td>1</td></tr>");
    let body = find(&dom, atom!("body"));
    assert_eq!(fragment_of(&mut dom, body, "<td>1</td>x"), "1x", "a cell outside a table is dropped");
}

#[test]
fn html_and_body_tags_in_a_fragment_add_no_elements() {
    let mut dom = parse("");
    let body = find(&dom, atom!("body"));
    assert_eq!(fragment_of(&mut dom, body, "<html><body class=a><p>1</p></body></html>"), "<p>1</p>");
    let html_el = find(&dom, atom!("html"));
    assert_eq!(fragment_of(&mut dom, html_el, "<p>1"), "<head></head><body><p>1</p></body>");
}

#[test]
fn templates_in_a_fragment_get_contents_of_the_inert_document() {
    let mut dom = parse("<div></div>");
    let div = find(&dom, atom!("div"));
    let parsed = dom.parse_fragment(div, b"<template><i>t</i></template>", true).unwrap();
    let t = dom.first_child(parsed.fragment).unwrap();
    let contents = dom.contents(t).unwrap();
    assert!(dom.is_inert_document(dom.owner_document(contents)));
    assert_eq!(html(&dom, t), "<i>t</i>");
    let held = dom.hold(parsed.fragment);
    dom.let_go(held);
    let mut freed = Vec::new();
    dom.free_dead(&mut freed);
    assert_eq!(dom.audit().mismatches(), 0, "{:?}", dom.audit());
}

#[test]
fn scripting_decides_how_noscript_parses() {
    let mut dom = parse("<div></div>");
    let div = find(&dom, atom!("div"));
    for (scripting, kind) in [(true, NodeKind::Text), (false, NodeKind::Element)] {
        let parsed = dom.parse_fragment(div, b"<noscript><p>x</p></noscript>", scripting).unwrap();
        let ns = dom.first_child(parsed.fragment).unwrap();
        assert_eq!(dom.kind(dom.first_child(ns).unwrap()), kind);
        let held = dom.hold(parsed.fragment);
        dom.let_go(held);
    }
}

#[test]
fn a_fragment_past_the_node_limit_is_incomplete() {
    let limits = Limits { nodes: 2100, ..Limits::default() };
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(Dom::with_limits(limits), Options::default());
    tok.feed(b"<div></div>");
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    let mut dom = b.into_sink();
    let div = find(&dom, atom!("div"));
    let parsed = dom.parse_fragment(div, "<i></i>".repeat(3000).as_bytes(), true).unwrap();
    assert!(!parsed.complete);
    let held = dom.hold(parsed.fragment);
    dom.let_go(held);
    let mut freed = Vec::new();
    dom.free_dead(&mut freed);
    assert!(dom.refused().is_none(), "the fragment's refusal is not kept for later edits");
    assert_eq!(dom.audit().mismatches(), 0, "{:?}", dom.audit());
}

#[test]
fn a_dom_one_node_below_its_limit_refuses_a_fragment() {
    let limits = Limits { nodes: 4096, ..Limits::default() };
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(Dom::with_limits(limits), Options::default());
    tok.feed(b"<div></div>");
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    let mut dom = b.into_sink();
    let div = find(&dom, atom!("div"));
    let doc = dom.document();
    let mut filler = Vec::new();
    while dom.node_count() < limits.nodes - 1 {
        let el = dom.new_element_in(doc, atom!("i")).unwrap();
        filler.push(dom.hold(el));
    }
    assert!(dom.parse_fragment(div, b"<b>x</b>", true).is_none());
    for f in filler {
        dom.let_go(f);
    }
    let mut freed = Vec::new();
    dom.free_dead(&mut freed);
    let parsed = dom.parse_fragment(div, b"<b>x</b>", true).unwrap();
    assert!(parsed.complete, "the room given back is room again");
    let held = dom.hold(parsed.fragment);
    dom.let_go(held);
    dom.free_dead(&mut freed);
    assert_eq!(dom.audit().mismatches(), 0, "{:?}", dom.audit());
}
