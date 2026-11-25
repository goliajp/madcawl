//! Custom properties at the sizes real stylesheets reach, and `var()`
//! values shared by the elements that match the same rule.

#[allow(dead_code)]
mod style;

use std::time::Duration;

use madcawl_css::style::{ElementStates, Prop};
use madcawl_html::atom;
use style::{Pair, by_id, check, page, parse, set, sheets};

/// The CPU time this thread has used: unlike the clock, it does not
/// grow while other processes have the machine.
fn thread_cpu() -> Duration {
    #[repr(C)]
    struct Timespec {
        sec: i64,
        nsec: i64,
    }
    unsafe extern "C" {
        fn clock_gettime(id: i32, t: *mut Timespec) -> i32;
    }
    #[cfg(target_os = "macos")]
    const THREAD_CPUTIME: i32 = 16;
    #[cfg(not(target_os = "macos"))]
    const THREAD_CPUTIME: i32 = 3;
    let mut t = Timespec { sec: 0, nsec: 0 };
    assert_eq!(unsafe { clock_gettime(THREAD_CPUTIME, &mut t) }, 0);
    Duration::new(t.sec as u64, t.nsec as u32)
}

fn css(p: &style::Page, id: &str, prop: &str) -> String {
    p.style(id).css(Prop::from_name(prop).expect("a property"))
}

#[test]
fn thousands_of_custom_properties_on_one_element_cascade_in_order() {
    // a theme rule on the root of tens of thousands of properties, as
    // design systems write them; looking each name up among those kept
    // takes a billion comparisons, sorting them a million
    let n = 30_000;
    let mut css = String::from("<!doctype html><html id=r><style>:root {");
    for i in 0..n {
        css.push_str(&format!(" --a{i}: {i}px; --b{i}: var(--a{i});"));
    }
    css.push_str(
        " } html:root { --a7: 70px; --a8: 80px !important } :root { --a8: 8px; --a9: 9px }",
    );
    css.push_str(
        " html { --a9: 90px !important } </style><p id=x style='--a9: 1px; --a10: 100px'></p>",
    );
    let t = thread_cpu();
    let p = page(&css);
    let root = p.el("r");
    let every = p.styles.custom_properties(root).len();
    let took = thread_cpu() - t;
    let get = |el, name: &str| p.styles.custom_property(el, name);
    assert_eq!(get(root, "--a7"), Some("70px"), "higher specificity");
    assert_eq!(get(root, "--a8"), Some("80px"), "important");
    assert_eq!(
        get(root, "--b7"),
        Some("70px"),
        "substituted from the winner"
    );
    assert_eq!(get(root, "--a9"), Some("90px"));
    assert_eq!(get(root, "--b29999"), Some("29999px"));
    let x = p.el("x");
    assert_eq!(get(x, "--a9"), Some("1px"), "the element's own wins");
    assert_eq!(get(x, "--a10"), Some("100px"));
    assert_eq!(get(x, "--b10"), Some("10px"), "substituted where declared");
    assert_eq!(every, 2 * n);
    // about 0.2 s in a debug build; 17 s when each name is looked up
    // among the ones before it
    assert!(took < Duration::from_secs(5), "{took:?} of CPU");
}

#[test]
fn one_rule_gives_each_set_its_own_value() {
    let p = page(
        "<!doctype html><style>i { width: var(--w); margin-inline-start: var(--w) }\
         #b { --w: 2px } #c { --w: 3px; direction: rtl }</style>\
         <div style='--w: 1px'><i id=a1></i><i id=a2></i></div>\
         <div id=b><i id=b1></i></div><div id=c><i id=c1></i><i id=c2 style='direction: ltr'></i></div>",
    );
    for (id, w) in [("a1", "1px"), ("a2", "1px"), ("b1", "2px"), ("c1", "3px")] {
        assert_eq!(css(&p, id, "width"), w, "#{id}");
    }
    assert_eq!(css(&p, "a1", "margin-left"), "1px");
    assert_eq!(
        css(&p, "c1", "margin-right"),
        "3px",
        "the side by direction"
    );
    assert_eq!(css(&p, "c1", "margin-left"), "0px");
    assert_eq!(css(&p, "c2", "margin-left"), "3px");
    assert_eq!(css(&p, "c2", "margin-right"), "0px");
}

#[test]
fn a_restyle_substitutes_its_own_style_attributes() {
    // a restyle parses style attributes again and may put them where
    // the run before had others
    let mut dom = parse(
        "<!doctype html><div style='--a: 1px; --b: 2px'><p id=p style='width: var(--a)'></p></div>",
    );
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let p = by_id(&dom, "p");
    for (i, v) in [
        "height: var(--a)",
        "width: var(--b)",
        "height: var(--b)",
        "width: var(--a)",
    ]
    .iter()
    .cycle()
    .take(12)
    .enumerate()
    {
        let ch = [set(&mut dom, p, atom!("style"), v)];
        check(&c, &dom, &mut s, &st, &ch);
        let want = if v.contains("--a") { "1px" } else { "2px" };
        let prop = if v.starts_with("width") {
            Prop::Width
        } else {
            Prop::Height
        };
        assert_eq!(s.get(p).expect("styled").css(prop), want, "step {i}: {v}");
    }
}

#[test]
fn every_property_in_a_cycle_is_invalid_whichever_is_resolved_first() {
    // --b is resolved before the cycle through it is closed when --x
    // comes first; names give the order, so each is tried first
    let names = [["x", "a", "b"], ["a", "b", "x"], ["b", "x", "a"]];
    for [x, a, b] in names {
        let css = format!(
            "<style>p {{ --{x}: var(--{a}) var(--{b}); --{a}: var(--{x}); --{b}: var(--{a}, fb);\
             --out: var(--{b}, ok) }}</style><p id=p></p>"
        );
        let p = page(&css);
        for n in [x, a, b] {
            let got = p.styles.custom_property(p.el("p"), &format!("--{n}"));
            assert_eq!(got, None, "--{n} with --{x} --{a} --{b}");
        }
        let out = p.styles.custom_property(p.el("p"), "--out");
        assert_eq!(out, Some("ok"), "outside the cycle");
    }
}

// css-variables-1 2.3: an edge for every var() in the value, the fallback
// argument included, so a reference in an unused fallback still closes a cycle
#[test]
fn a_property_referring_to_itself_is_invalid() {
    let p = page(
        "<style>p { --a: var(--a); --b: 1 var(--b, 2); --c: var(--b, 3) var(--a, 4) }</style><p id=p></p>",
    );
    let get = |n: &str| p.styles.custom_property(p.el("p"), n);
    assert_eq!(get("--a"), None);
    assert_eq!(get("--b"), None, "through its own fallback");
    assert_eq!(get("--c"), Some("3 4"));
}

#[test]
fn a_cycle_through_a_fallback_is_invalid() {
    // --a falls back to --b because --m is missing; --b leads back to --a
    for first in ["a", "z"] {
        let other = if first == "a" { "z" } else { "a" };
        let css = format!(
            "<style>p {{ --{first}: var(--m, var(--{other})); --{other}: x var(--{first});\
             --k: var(--m, var(--{first}, ok)) }}</style><p id=p></p>"
        );
        let p = page(&css);
        let get = |n: &str| p.styles.custom_property(p.el("p"), &format!("--{n}"));
        assert_eq!(get(first), None, "--{first} first");
        assert_eq!(get(other), None, "--{first} first");
        assert_eq!(get("k"), Some("ok"));
    }
}
