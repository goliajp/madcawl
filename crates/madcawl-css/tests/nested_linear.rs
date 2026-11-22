//! Many sibling rules nested in one style rule: each is tried as a
//! declaration first, and that try must not read the rest of the block,
//! or the parse grows with the square of the block.

use std::time::{Duration, Instant};

use madcawl_css::{Rule, Stylesheet};

fn nested_count(src: String) -> (usize, Duration) {
    let t = Instant::now();
    let sheet = Stylesheet::parse(src).unwrap();
    let n = match &sheet.rules()[0] {
        Rule::Style(s) => s.rules.len(),
        r => panic!("not a style rule: {r:?}"),
    };
    (n, t.elapsed())
}

#[test]
fn sibling_nested_rules_parse_in_linear_time() {
    // read the whole block once per rule and 50 000 rules take tens of
    // seconds; read it once and they take milliseconds
    for unit in ["p{}", "a:hover{}", "b c{}"] {
        let src = format!("x{{{}}}", unit.repeat(50_000));
        let (n, took) = nested_count(src);
        assert_eq!(n, 50_000, "{unit}");
        assert!(took < Duration::from_secs(3), "{unit}: {took:?}");
    }
}
