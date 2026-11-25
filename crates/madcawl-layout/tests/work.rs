//! How much work layout does grows with the document, not with the product of its nesting or its siblings:
//! boxes laid out and float and inline box entries looked at, counted rather than timed.

mod common;

use common::*;
use madcawl_layout::{Counters, Layout};

// without reuse nested flex containers take time exponential in their depth: the differential check runs on
// a shallower copy
const SHALLOW: usize = 8;

fn counters(html: &str) -> Counters {
    let dom = parse(html);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    assert!(l.fragments().is_some());
    l.counters()
}

fn nested(open: &str, depth: usize) -> String {
    format!(
        "<!doctype html><body>{}x{}",
        open.repeat(depth),
        "</div>".repeat(depth)
    )
}

#[test]
fn nested_flex_containers_lay_each_box_out_a_bounded_number_of_times() {
    for open in [
        "<div style='display:flex'>",
        "<div style='display:flex;flex-direction:column'>",
        "<div style='display:flex'><div>",
        "<div style='display:flex;flex-direction:column;align-items:center'>",
    ] {
        let depth = 30;
        same_without_reuse(&nested(open, SHALLOW), 1280.0, 800.0);
        let c = counters(&nested(open, depth));
        let boxes = depth * open.matches("<div").count() + 4;
        assert!(
            c.laid <= 3 * boxes as u64,
            "{open}: {c:?} for {boxes} boxes"
        );
    }
}

#[test]
fn nested_flex_reading_percentages_of_the_height_lays_each_box_out_a_bounded_number_of_times() {
    let cases = [
        ("<div style='display:flex'>", "height:50%"),
        (
            "<div style='display:flex;flex-direction:column'>",
            "height:50%",
        ),
        ("<div style='display:flex;min-height:100%'>", ""),
        (
            "<div style='display:flex;flex-direction:column;min-height:100%'>",
            "",
        ),
        ("<div style='display:flex;row-gap:1%'>", ""),
        (
            "<div style='display:flex;flex-direction:column;min-height:10%'>",
            "",
        ),
    ];
    // the root and body heights definite or not
    let roots = [
        "<html style='height:100%'><body style='height:100%'>",
        "<body>",
    ];
    for ((open, leaf), root) in cases.iter().flat_map(|c| roots.map(|r| (c, r))) {
        let html = |depth: usize| {
            format!(
                "<!doctype html>{root}{}<div style='{leaf}'>x</div>{}",
                open.repeat(depth),
                "</div>".repeat(depth)
            )
        };
        same_without_reuse(&html(SHALLOW), 1280.0, 800.0);
        // twice the depth, about twice the work: no factor per level
        let laid = |d: usize| counters(&html(d)).laid;
        let (a, b, c) = (laid(11), laid(22), laid(44));
        assert!(
            b <= 5 * a / 2 && c <= 5 * b / 2,
            "{open} {leaf}: laid {a}, {b}, {c} at depths 11, 22, 44"
        );
        let depth = 22;
        let c = counters(&html(depth));
        let boxes = depth + 5;
        // measuring an item leaves out the stretching below it, so each box is measured and laid out once
        // per containing block height it is given
        assert!(
            c.laid <= 4 * boxes as u64,
            "{open} {leaf}: {c:?} for {boxes} boxes"
        );
    }
}

#[test]
fn many_floats_among_text_look_at_a_bounded_number_of_floats() {
    let work = |n: usize| {
        let html = format!(
            "<!doctype html><body><p>{}</p>",
            "<span style='float:left;width:3px;height:3px'></span>word word ".repeat(n)
        );
        same_without_reuse(&html, 1280.0, 800.0);
        counters(&html).float_visits
    };
    let (a, b) = (work(500), work(2000));
    // four times the floats, not sixteen times the work
    assert!(b <= 6 * a.max(1), "{a} then {b}");
}

#[test]
fn many_inline_boxes_in_one_paragraph_are_looked_at_a_bounded_number_of_times() {
    let work = |n: usize| {
        let html = format!(
            "<!doctype html><body><p>{}</p>",
            "<a>word</a> word ".repeat(n)
        );
        same_without_reuse(&html, 1280.0, 800.0);
        counters(&html).inline_box_visits
    };
    let (a, b) = (work(500), work(2000));
    assert!(b <= 6 * a.max(1), "{a} then {b}");
}

#[test]
fn nested_columns_whose_minimum_is_a_percentage_of_the_height_stay_polynomial() {
    // each container's definite height reaches the boxes below it as another containing block height, so a
    // box at depth d is asked for in up to about 3d spaces: quadratic in the depth, never exponential
    let col = "<div style='display:flex;flex-direction:column;min-height:60%;padding:2px'>";
    let cases = [
        (format!("{col}x"), 1),
        (
            format!("{col}<div style='min-height:60%;padding:2px'>x</div>"),
            2,
        ),
        (
            format!("{col}<div style='display:flex;flex-direction:column;min-height:60%'>x</div>"),
            2,
        ),
    ];
    for (open, per_level) in &cases {
        let html = |depth: usize| {
            format!(
                "<!doctype html><html style='height:100%'><body style='height:100%;margin:0;\
                 font:16px/20px Menlo'>{}x{}",
                open.repeat(depth),
                "</div>".repeat(depth)
            )
        };
        same_without_reuse(&html(SHALLOW), 1280.0, 800.0);
        let depth = 32;
        let boxes = (per_level * depth + 4) as u64;
        let (half, full) = (counters(&html(depth / 2)).laid, counters(&html(depth)).laid);
        assert!(
            full <= boxes * boxes,
            "{open}: laid {full} for {boxes} boxes"
        );
        // twice the depth: about four times the work, not its square
        assert!(full <= 5 * half, "{open}: laid {half} then {full}");
    }
}
