//! Component value trees as deep as the input makes them.

use madcawl_css::{Span, parse_component_value_list};

#[test]
fn a_deep_tree_drops_without_recursing() {
    let src = "(".repeat(2_000_000);
    let tree = parse_component_value_list(&src, Span::new(0, src.len()));
    assert_eq!(tree.len(), 1);
    drop(tree);
    let src = "[{(a".repeat(500_000);
    drop(parse_component_value_list(&src, Span::new(0, src.len())));
}
