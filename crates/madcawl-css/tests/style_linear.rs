//! The cascade's cost grows with the page, not faster: structural
//! pseudo-classes over many siblings, and ids and classes in quirks
//! mode, where they match ASCII case-insensitively.

#[allow(dead_code)]
mod style;

use std::time::{Duration, Instant};

use madcawl_css::style::Display;
use madcawl_dom::NodeKind;
use madcawl_html::atom;
use style::page;

/// How many `<i>`, `<b>` and `<p>` elements are `display: none`.
fn hidden_count(p: &style::Page) -> usize {
    let names = [atom!("i"), atom!("b"), atom!("p")];
    p.dom
        .descendants(p.dom.document())
        .filter(|&n| p.dom.kind(n) == NodeKind::Element)
        .filter(|&n| p.dom.local_name(n).is_some_and(|l| names.contains(&l)))
        .filter(|&n| p.styles.get(n).is_some_and(|s| s.display == Display::None))
        .count()
}

#[test]
fn nth_child_over_many_siblings_is_linear() {
    // walk the siblings for each element and 40 000 rows take
    // billions of steps; number them once and they take milliseconds
    let n = 40_000;
    let html = format!(
        "<!doctype html><style>i:nth-child(odd), b:nth-last-of-type(4n+1) {{ display: none }}</style>\
         <div>{}</div>",
        "<i></i><b></b>".repeat(n / 2)
    );
    let t = Instant::now();
    let p = page(&html);
    let took = t.elapsed();
    // every <i> is at an odd position; every fourth <b> from the end
    assert_eq!(hidden_count(&p), n / 2 + n / 8);
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn quirks_mode_ids_and_classes_are_bucketed() {
    // try every rule on every element, as an unbucketed rule is, and
    // 5 000 of each take 25 million comparisons
    let n = 5_000;
    let mut html = String::from("<style>");
    for i in 0..n {
        html.push_str(&format!(".C{i}, #I{i} {{ display: none }}"));
    }
    html.push_str("</style>");
    for i in 0..n {
        html.push_str(&format!(
            "<p class=c{i}></p><p id=i{i}></p><p class=x{i}></p>"
        ));
    }
    let t = Instant::now();
    let p = page(&html);
    let took = t.elapsed();
    assert_eq!(hidden_count(&p), 2 * n);
    assert!(took < Duration::from_secs(5), "{took:?}");
}
