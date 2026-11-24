//! Custom properties and `var()`.

#[allow(dead_code)]
mod style;

use madcawl_css::style::Display;
use style::page;

#[test]
fn inherited_and_substituted() {
    let p = page(
        "<style>:root { --hide: none; --shown: block } .a { display: var(--hide) }\
         .b { --hide: flex } .c { display: var(--shown) }</style>\
         <div id=a class=a></div><div class=b><span id=b class=a></span></div>\
         <span id=c class=c></span>",
    );
    assert_eq!(p.display("a"), Display::None);
    assert_eq!(
        p.display("b"),
        Display::Flex,
        "the nearest declaration wins"
    );
    assert_eq!(p.display("c"), Display::Block);
    assert_eq!(p.styles.custom_property(p.el("b"), "--hide"), Some("flex"));
    assert_eq!(p.styles.custom_property(p.el("a"), "--hide"), Some("none"));
    assert_eq!(
        p.styles.custom_properties(p.el("c")),
        [("--hide", "none"), ("--shown", "block")]
    );
}

#[test]
fn fallbacks_and_invalid_values() {
    let p = page(
        "<style>#a { display: var(--missing, none) } #b { display: var(--missing) }\
         #c { --x: frob; display: var(--x) } #d { visibility: hidden } #d > i { visibility: var(--nope) }\
         #e { display: var(--m, var(--n, grid)) } #f { --v: var(--w); --w: var(--v); display: var(--v, flex) }\
         #g { --k: initial; display: var(--k, table) }</style>\
         <div id=a></div><div id=b></div><div id=c></div><div id=d><i id=di></i></div>\
         <div id=e></div><div id=f></div><div id=g></div>",
    );
    assert_eq!(p.display("a"), Display::None);
    assert_eq!(
        p.display("b"),
        Display::Inline,
        "invalid at computed-value time: unset"
    );
    assert_eq!(p.display("c"), Display::Inline);
    assert!(!p.visible("di"), "an unset inherited property inherits");
    assert_eq!(p.display("e"), Display::Grid);
    assert_eq!(
        p.display("f"),
        Display::Flex,
        "a property in a cycle is guaranteed-invalid"
    );
    assert_eq!(p.styles.custom_property(p.el("f"), "--v"), None);
    assert_eq!(p.display("g"), Display::Table);
}

#[test]
fn custom_values_resolve_their_own_var() {
    let p = page(
        "<style>:root { --a: in; --b: var(--a)line } #x { --a: blo; --c: var(--a)ck; display: var(--c) }\
         #y { opacity: var(--o, 50%) } #z { --sp:   spaced  value  }</style>\
         <p id=x></p><p id=y></p><p id=z></p>",
    );
    assert_eq!(p.styles.custom_property(p.el("x"), "--c"), Some("block"));
    assert_eq!(p.display("x"), Display::Block);
    assert_eq!(
        p.styles.custom_property(p.el("x"), "--b"),
        Some("inline"),
        "computed on :root"
    );
    assert_eq!(p.style("y").opacity(), 0.5);
    assert_eq!(
        p.styles.custom_property(p.el("z"), "--sp"),
        Some("spaced  value")
    );
}

#[test]
fn substitution_keeps_tokens_apart() {
    let p = page("<style>#a { --d: inline; display: var(--d)-block }</style><p id=a></p>");
    assert_eq!(
        p.display("a"),
        Display::Inline,
        "two tokens, not `inline-block`: invalid"
    );
}

#[test]
fn long_chains_stay_correct() {
    let mut html = String::from("<style>:root { --root: none }");
    for i in 0..20 {
        html.push_str(&format!(".l{i} {{ --v{i}: {i} }}"));
    }
    html.push_str("#leaf { display: var(--root) }</style>");
    for i in 0..20 {
        html.push_str(&format!("<div class=l{i}>"));
    }
    html.push_str("<span id=leaf></span>");
    let p = page(&html);
    assert_eq!(p.display("leaf"), Display::None);
    let all = p.styles.custom_properties(p.el("leaf"));
    assert_eq!(all.len(), 21);
    assert_eq!(p.styles.custom_property(p.el("leaf"), "--v0"), Some("0"));
    assert_eq!(p.styles.custom_property(p.el("leaf"), "--v19"), Some("19"));
}

#[test]
fn custom_properties_cascade_too() {
    let p = page(
        "<style>#a { --x: none !important } #a { --x: block } .b { --y: grid }</style>\
         <p id=a class=b style='--y: flex; display: var(--y)'></p>",
    );
    assert_eq!(p.styles.custom_property(p.el("a"), "--x"), Some("none"));
    assert_eq!(p.display("a"), Display::Flex);
}
