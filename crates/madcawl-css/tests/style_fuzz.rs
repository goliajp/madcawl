//! Style attributes made of random pieces of CSS, huge numbers among
//! them, style a page without a panic, and `PathKey` agrees with the
//! full path on every element.

#[allow(dead_code)]
mod style;

use style::{cascade, differences, parse, sheets};

const PIECES: &[&str] = &[
    "calc(",
    ")",
    "1e38",
    "px",
    "%",
    "var(--x",
    ",",
    "--x:",
    ";",
    "inherit",
    "margin",
    ":",
    "-",
    "*",
    "/",
    "0",
    "inset",
    "line-height",
    "font",
    "border",
    "auto",
    " ",
    "!important",
    "revert",
    "em",
    "rem",
    "min(",
    "max(",
    "clamp(",
    "1e-38",
    "nan",
    "infinity",
    "inset-block",
    "font-size",
    "100000000",
    "unset",
    "{",
    "}",
    "\\",
    "'",
    "url(",
    "#",
    "rgb(",
    "attr(",
    "env(",
];

#[test]
fn random_style_attributes_style_without_a_panic() {
    let mut seed = 1u64;
    let mut html = String::from("<!doctype html>");
    for _ in 0..3000 {
        let mut s = String::new();
        for _ in 0..12 {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            s.push_str(PIECES[(seed >> 33) as usize % PIECES.len()]);
        }
        html.push_str(&format!(
            "<div style=\"{}\"><span>x</span></div>",
            s.replace('"', "")
        ));
    }
    let dom = parse(&html);
    let list = sheets(&dom);
    let mut c = cascade(&list);
    c.set_verify(true);
    let keyed = c.compute(&dom);
    assert_eq!(keyed.stats().mismatches, 0);
    c.set_path_key(false);
    assert_eq!(differences(&dom, &keyed, &c.compute(&dom)), 0);
}
