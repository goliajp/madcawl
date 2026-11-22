//! Moving a subtree within the same document costs about the same however big
//! the subtree is: its in-document bits do not change, so they are not
//! rewritten.

use std::time::{Duration, Instant};

use madcawl_dom::{Dom, NodeId};
use madcawl_html::{Attrs, Namespace, TreeSink, atom};

fn setup(size: usize) -> (Dom, NodeId, NodeId, NodeId) {
    let mut dom = Dom::new();
    let doc = dom.document();
    let body = dom.create_element(Namespace::Html, atom!("body"), Attrs::empty());
    dom.append(doc, body);
    let (a, b) = (
        dom.create_element(Namespace::Html, atom!("div"), Attrs::empty()),
        dom.create_element(Namespace::Html, atom!("div"), Attrs::empty()),
    );
    dom.append(body, a);
    dom.append(body, b);
    let moved = dom.create_element(Namespace::Html, atom!("section"), Attrs::empty());
    dom.append(a, moved);
    for _ in 0..size {
        let x = dom.create_element(Namespace::Html, atom!("span"), Attrs::empty());
        dom.append(moved, x);
    }
    (dom, a, b, moved)
}

fn fastest(size: usize) -> Duration {
    let (mut dom, a, b, moved) = setup(size);
    (0..5)
        .map(|_| {
            let t = Instant::now();
            for i in 0..2_000 {
                dom.append(if i % 2 == 0 { b } else { a }, moved);
            }
            t.elapsed()
        })
        .min()
        .unwrap()
}

#[test]
fn moving_a_subtree_within_the_document_does_not_walk_it() {
    let (small, large) = (fastest(0), fastest(4_000));
    let r = large.as_secs_f64() / small.as_secs_f64();
    println!("2000 moves: 1 node {small:?}, 4001 nodes {large:?}, ratio {r:.2}");
    assert!(r < 3.0, "moves cost grows with the subtree: {r:.2}");
}
