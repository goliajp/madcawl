//! The DOM's two limits: past its node limit it creates no node and the
//! parser stops with an error; past its byte limit strings are cut and
//! the parser stops the same way.

use madcawl_dom::{Dom, HEADROOM, Limits, NodeKind, Refused};
use madcawl_html::{Options, Step, Tokenizer, TreeBuilder};

fn parse(dom: Dom, input: &[u8]) -> Dom {
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(dom, Options::default());
    for chunk in input.chunks(4096) {
        tok.feed(chunk);
        assert_eq!(tok.run(&mut b), Step::NeedInput);
    }
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    b.into_sink()
}

fn limited(nodes: u32, bytes: usize) -> Dom {
    Dom::with_limits(Limits { nodes, bytes })
}

#[test]
fn parsing_stops_with_an_error_at_the_node_limit() {
    let dom = parse(limited(4096, 1 << 20), &b"<p>x".repeat(100_000));
    assert!(
        dom.parser_stopped(),
        "the parser reported document-too-large"
    );
    assert_eq!(dom.refused(), Some(Refused::Nodes));
    assert!(dom.node_count() <= 4096);
    assert!(
        dom.node_count() + HEADROOM > 4096,
        "it stopped near the limit, not before"
    );
    let a = dom.audit();
    assert_eq!(a.mismatches(), 0, "{a:?}");
    assert_eq!(a.nodes, dom.node_count() as u64);
}

#[test]
fn a_document_under_the_limit_is_not_stopped() {
    let dom = parse(limited(4096, 1 << 20), &b"<p>x".repeat(100));
    assert!(!dom.parser_stopped());
    assert_eq!(dom.refused(), None);
}

#[test]
fn parsing_stops_with_an_error_at_the_byte_limit() {
    let mut input = b"<p title=abcdefgh>".repeat(10).to_vec();
    input.extend(std::iter::repeat_n(b'y', 50_000));
    input.extend_from_slice(&b"<p>z".repeat(1000));
    let dom = parse(limited(1 << 20, 10_000), &input);
    assert!(dom.parser_stopped());
    assert_eq!(dom.refused(), Some(Refused::Bytes));
    assert_eq!(dom.bytes(), 10_000);
    // the long text node was cut to what was left
    let doc = dom.document();
    let longest = dom
        .descendants(doc)
        .filter(|&n| dom.kind(n) == NodeKind::Text)
        .map(|n| dom.text(n).unwrap().len())
        .max()
        .unwrap();
    // each p: 16 bytes of block head, 16 of attribute slot, 8 of value
    assert_eq!(longest, 10_000 - 10 * (16 + 16 + 8));
    assert!(
        !dom.descendants(doc)
            .any(|n| dom.text(n).is_some_and(|t| t.eq_str("z")))
    );
    assert_eq!(dom.audit().mismatches(), 0);
}

#[test]
fn a_cut_falls_on_a_character_boundary() {
    // "é" is two bytes; a limit landing between them keeps neither
    let mut input = b"<p>".to_vec();
    input.extend("é".repeat(10_000).as_bytes());
    let dom = parse(limited(1 << 20, 10_001), &input);
    let t = dom
        .descendants(dom.document())
        .find_map(|n| dom.text(n))
        .unwrap();
    assert_eq!(t.len(), 5000);
    assert!(t.units().all(|u| u == 0xE9));
}

#[test]
fn cloning_into_selectedcontent_stops_at_the_node_limit() {
    // the clone happens when the option closes, inside one token, and
    // can make many nodes: it stops itself, leaving the parser's headroom
    let mut input =
        b"<select><button><selectedcontent></selectedcontent></button><option>".to_vec();
    input.extend_from_slice(&b"<span>x</span>".repeat(1500));
    input.extend_from_slice(b"</option></select>");
    let dom = parse(limited(4096, 1 << 20), &input);
    assert_eq!(dom.refused(), Some(Refused::Nodes));
    assert!(dom.node_count() <= 4096 - HEADROOM, "{}", dom.node_count());
    assert_eq!(dom.audit().mismatches(), 0);
}

#[test]
fn a_clone_leaves_room_for_the_token_that_triggered_it() {
    // the option closes inside the start tag of the next option, whose
    // own element still needs a node after the clone
    let mut input =
        b"<select><button><selectedcontent></selectedcontent></button><option>".to_vec();
    input.extend_from_slice(&b"<span>x</span>".repeat(1500));
    input.extend_from_slice(b"<option>");
    let dom = parse(limited(4096, 1 << 20), &input);
    assert_eq!(dom.refused(), Some(Refused::Nodes));
    assert_eq!(dom.audit().mismatches(), 0);
}

#[test]
fn one_token_reopening_formatting_elements_twice_fits_the_headroom() {
    // 256 formatting elements closed by </p> stay in the list; the table
    // text is flushed by the next token, which reopens all of them for
    // the text and then again for its own element
    let mut tail = b"<p><a id=0>".to_vec();
    for i in 1..256 {
        tail.extend_from_slice(format!("<b id={i}>").as_bytes());
    }
    tail.extend_from_slice(b"</p><table>");
    let mut input = b"<!---->".repeat(2000);
    input.extend_from_slice(&tail);
    let before = parse(Dom::new(), &input).node_count();
    input.extend_from_slice(b"x<a>");
    let dom = parse(limited(before + HEADROOM, 1 << 20), &input);
    assert_eq!(dom.audit().mismatches(), 0);
}
