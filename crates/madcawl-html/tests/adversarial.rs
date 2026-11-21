//! Inputs built to make a parser do quadratic work, each parsed at two
//! sizes, N and 4N. Linear work takes about four times as long at 4N,
//! quadratic about sixteen; the test asks for less than six, on the
//! fastest of five runs at each size. Run it with `--release` on a
//! machine that is not busy: the ratio of a debug build measures the
//! debug build, and one of a loaded machine measures the load.

use std::time::{Duration, Instant};

use madcawl_html::{Options, Step, Tokenizer, TreeBuilder, TreeSink};

#[path = "support/dom.rs"]
mod dom;

use dom::Dom;

fn parse(input: &[u8]) -> Dom {
    let mut tok = Tokenizer::new();
    let mut builder = TreeBuilder::new(Dom::new(), Options::default());
    tok.feed(input);
    tok.end();
    assert_eq!(tok.run(&mut builder), Step::Done);
    builder.into_sink()
}

fn fastest(input: &[u8]) -> Duration {
    (0..5)
        .map(|_| {
            let t = Instant::now();
            std::hint::black_box(parse(input));
            t.elapsed()
        })
        .min()
        .unwrap()
}

/// The ratio of the time at 4N to the time at N.
fn ratio(name: &str, n: usize, make: impl Fn(usize) -> Vec<u8>) -> f64 {
    let (small, large) = (make(n), make(4 * n));
    let (a, b) = (fastest(&small), fastest(&large));
    let r = b.as_secs_f64() / a.as_secs_f64();
    println!("{name}: N={n} {a:?}, 4N={} {b:?}, 4N/N {r:.2}", 4 * n);
    r
}

const LIMIT: f64 = 6.0;

#[test]
fn deep_nesting() {
    let r = ratio("deep nesting", 20_000, |n| b"<div>".repeat(n));
    assert!(r < LIMIT, "deep nesting: 4N/N = {r:.2}");
}

#[test]
fn deep_nesting_stops_at_the_depth_limit() {
    let dom = parse(&b"<div>".repeat(2000));
    let depth = |mut n: usize| {
        let mut d = 0;
        while let Some(p) = dom.parent(n) {
            n = p;
            d += 1;
        }
        d
    };
    let deepest = (0..dom.nodes.len()).map(depth).max().unwrap();
    // as Blink: nesting stops once 513 elements are open (html and 512
    // more), so the deepest element is 514 below the document
    assert_eq!(deepest, 514, "levels below the document");
    let mut out = Vec::new();
    dom::print(&dom, 0, 0, &mut out);
    assert!(out.len() > 2000 * 5, "every div is still in the tree");
}

#[test]
fn many_attributes() {
    // kept small for the reason given at many_tag_names
    let r = ratio("many attributes", 5_000, |n| {
        let mut s = b"<a".to_vec();
        for i in 0..n {
            s.extend_from_slice(format!(" a{i}=x").as_bytes());
        }
        s.push(b'>');
        s
    });
    assert!(r < LIMIT, "many attributes: 4N/N = {r:.2}");
}

#[test]
fn many_formatting_elements() {
    let r = ratio("many formatting elements", 5_000, |n| {
        (0..n)
            .flat_map(|i| format!("<b x={i}>").into_bytes())
            .collect()
    });
    assert!(r < LIMIT, "many formatting elements: 4N/N = {r:.2}");
}

#[test]
fn many_tag_names() {
    // small: past a few tens of thousands of names the table outgrows
    // the cache and each lookup costs more, which measures the memory
    // rather than the algorithm (5.5 to 6.0 at 20 000 on an idle
    // machine, with every doubling 2.2 times the time)
    let r = ratio("many tag names", 5_000, |n| {
        (0..n)
            .flat_map(|i| format!("<t{i}></t{i}>").into_bytes())
            .collect()
    });
    assert!(r < LIMIT, "many tag names: 4N/N = {r:.2}");
}
