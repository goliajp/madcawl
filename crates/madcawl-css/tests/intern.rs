//! Equal computed values share one place in the table of values, however
//! many different values the table grows to hold.

#[allow(dead_code)]
mod style;

use style::page;

#[test]
fn equal_values_reached_by_different_declarations_share_one_place() {
    // the two spellings give the elements different style attributes, so
    // neither reuses the other's values by key: both are computed and
    // interned
    let n = 700;
    let mut html = String::from("<!doctype html><body>");
    for i in 0..n {
        html += &format!("<p id=a{i} style='position:relative;z-index:{i}'></p>");
        html += &format!("<p id=b{i} style='z-index: {i}; position: relative'></p>");
    }
    let p = page(&html);
    let base = page("<!doctype html><body><p></p>").styles.distinct();
    assert!(p.styles.distinct() >= n);
    assert!(p.styles.distinct() <= n + base);
    for i in 0..n {
        let (a, b) = (p.style(&format!("a{i}")), p.style(&format!("b{i}")));
        assert_eq!(a.place.z_index, Some(i as i32));
        assert!(
            std::ptr::eq(a, b),
            "#a{i} and #b{i} have equal values in two places"
        );
    }
    assert!(!std::ptr::eq(p.style("a1"), p.style("a2")));
}
