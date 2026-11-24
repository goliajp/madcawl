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

#[test]
fn an_unclosed_function_in_a_fallback_ends_with_the_value() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let p = page(
            "<p id=a style='display: var(--a, f('></p><p id=b></p>\
             <style>#b{display:var(--a,f(}",
        );
        tx.send((p.display("a"), p.display("b"))).unwrap();
    });
    let got = rx
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("substitution ends");
    assert_eq!(got, (Display::Inline, Display::Inline));
}

#[test]
fn substitution_has_a_length_limit() {
    let mut css = String::from(":root { --a0: x;");
    for i in 1..=26 {
        css.push_str(&format!(" --a{i}: var(--a{j})var(--a{j});", j = i - 1));
    }
    css.push_str(" display: var(--a26, block) }");
    let p = page(&format!("<html id=r><style>{css}</style>"));
    let len = |n: &str| p.styles.custom_property(p.el("r"), n).map(str::len);
    assert_eq!(len("--a10"), Some(1 << 10));
    assert_eq!(len("--a20"), Some(1 << 20));
    assert_eq!(len("--a21"), None, "over the limit: guaranteed-invalid");
    assert_eq!(len("--a26"), None);
    assert_eq!(p.display("r"), Display::Block);
}

#[test]
fn a_cycle_is_invalid_whatever_its_fallbacks() {
    let p = page(
        "<style>#a { --a: var(--b, x); --b: var(--a); --c: var(--a,ok); display: var(--a, block) }\
         #b { --a: var(--a, x) }</style><p id=a></p><p id=b></p>",
    );
    let get = |id: &str, n: &str| p.styles.custom_property(p.el(id), n);
    assert_eq!(get("a", "--a"), None);
    assert_eq!(get("a", "--b"), None);
    assert_eq!(
        get("a", "--c"),
        Some("ok"),
        "outside the cycle: the fallback"
    );
    assert_eq!(p.display("a"), Display::Block);
    assert_eq!(get("b", "--a"), None);
}

#[test]
fn var_needs_a_custom_property_name() {
    let p = page(
        "<style>#a { display: block; display: var(foo, none) }\
         #b { --x: ok; --x: var(bar, no); display: var(--x, var(baz)) }\
         #c { display: grid; display: var(--m, var(nope, none)) }</style>\
         <p id=a></p><p id=b></p><p id=c></p>",
    );
    assert_eq!(p.display("a"), Display::Block, "invalid when parsed");
    assert_eq!(p.styles.custom_property(p.el("b"), "--x"), Some("ok"));
    assert_eq!(p.display("b"), Display::Block, "the user agent's");
    assert_eq!(p.display("c"), Display::Grid, "in a fallback too");
}

#[test]
fn two_dashes_alone_are_reserved() {
    let p = page(
        "<style>#a { display: block; display: var(--, none) }\
         #b { --: flex; display: var(--m, grid) }</style><p id=a></p><p id=b></p>",
    );
    assert_eq!(p.display("a"), Display::Block, "invalid when parsed");
    assert_eq!(p.display("b"), Display::Grid);
    assert!(p.styles.custom_properties(p.el("b")).is_empty());
}

#[test]
fn a_fallback_loses_the_whitespace_at_its_ends() {
    let p = page(
        "<style>#a { --c: var(--a,   ok  ); --d: var(--a,); --e: var(--a, f( x ) );\
         --f: var(--a,\t1 2\n) }</style><p id=a></p>",
    );
    let get = |n: &str| p.styles.custom_property(p.el("a"), n);
    assert_eq!(get("--c"), Some("ok"));
    assert_eq!(get("--d"), Some(""));
    assert_eq!(get("--e"), Some("f( x )"));
    assert_eq!(get("--f"), Some("1 2"));
}

#[test]
fn doubling_values_on_every_element_stay_bounded() {
    let mut css = String::from("<style>* { --a0: x;");
    for i in 1..=20 {
        css.push_str(&format!(" --a{i}: var(--a{j})var(--a{j});", j = i - 1));
    }
    css.push_str(" }</style>");
    let mut html = css;
    for i in 0..3000 {
        html.push_str(&format!("<p id=p{i}></p>"));
    }
    html.push_str(&"<div>".repeat(200));
    html.push_str("<i id=deep></i>");
    let started = std::time::Instant::now();
    let p = page(&html);
    let elapsed = started.elapsed();
    let len = |id: &str| p.styles.custom_property(p.el(id), "--a20").map(str::len);
    assert_eq!(len("p0"), Some(1 << 20));
    assert_eq!(len("p2999"), Some(1 << 20), "siblings share one set");
    assert_eq!(
        len("deep"),
        None,
        "past the run's budget: guaranteed-invalid"
    );
    assert!(elapsed.as_secs() < 10, "took {elapsed:?}");
}

#[test]
fn a_chain_of_copies_draws_on_one_budget() {
    let mut css = String::from("<style>i { --a0: x;");
    for i in 1..=20 {
        css.push_str(&format!(" --a{i}: var(--a{j})var(--a{j});", j = i - 1));
    }
    css.push_str(" --b0: var(--a20);");
    for i in 1..=100 {
        css.push_str(&format!(" --b{i}: var(--b{j});", j = i - 1));
    }
    css.push_str(" }</style><i id=x></i>");
    let p = page(&css);
    let held: usize = (0..=100)
        .filter_map(|i| {
            p.styles
                .custom_property(p.el("x"), &format!("--b{i}"))
                .map(str::len)
        })
        .sum();
    assert!(held <= 66 << 20, "copies hold {held} bytes");
    assert_eq!(p.styles.custom_property(p.el("x"), "--b100"), None);
}
