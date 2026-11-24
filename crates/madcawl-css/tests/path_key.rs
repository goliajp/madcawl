//! `PathKey`: elements with the same chain of what selectors see share
//! one computation, and the shared values are the ones the full path
//! gives, for every kind of selector that looks past the element and
//! its ancestors.

#[allow(dead_code)]
mod style;

use style::{cascade, differences, parse, sheets};

/// Style `html` with and without `PathKey`, every hit checked; the
/// share of elements styled by key.
#[track_caller]
fn same_both_ways(html: &str) -> f64 {
    let dom = parse(html);
    let list = sheets(&dom);
    let mut c = cascade(&list);
    c.set_verify(true);
    let keyed = c.compute(&dom);
    let st = keyed.stats();
    assert_eq!(
        st.mismatches, 0,
        "a hit differs from the full path at {:?}",
        st.first_mismatch
    );
    assert_eq!(st.verified, st.hits);
    c.set_path_key(false);
    let full = c.compute(&dom);
    assert_eq!(full.stats().hits, 0);
    assert_eq!(differences(&dom, &keyed, &full), 0);
    st.hits as f64 / st.elements as f64
}

#[test]
fn repeated_structure_is_two_lookups_per_element() {
    let rows = "<tr><td class=k>a</td><td>b</td></tr>".repeat(500);
    let rate = same_both_ways(&format!(
        "<!doctype html><style>.k {{ color: blue }} td {{ padding: 2px }}</style><table>{rows}</table>"
    ));
    assert!(rate > 0.95, "{rate}");
}

#[test]
fn position_and_siblings_are_revalidated() {
    let items = "<li>x</li>".repeat(30);
    same_both_ways(&format!(
        "<!doctype html><style>li:nth-child(3n+1) {{ color: red }} li:first-child + li {{ margin: 3px }} \
         li:last-child {{ display: none }} li ~ li:nth-last-of-type(2) {{ float: left }} \
         ul:has(> li:empty) li {{ padding: 1px }} p:empty {{ margin: 9px }}</style>\
         <ul>{items}</ul><ul><li></li>{items}</ul><p></p><p>x</p>"
    ));
}

#[test]
fn ancestors_attributes_and_states_count() {
    same_both_ways(
        "<!doctype html><style>[data-x=a] span { color: red } [data-x] i { color: blue } \
         div:not([lang|=en]) b { margin: 1px } :lang(fr) b { padding: 2px } a:any-link span { float: left } \
         input:checked + label { color: green } #t span { display: block } .on > span { opacity: 0.5 }</style>\
         <div data-x=a><span>1</span><i>2</i></div><div data-x=b><span>1</span><i>2</i></div><div><span>1</span><i>2</i></div>\
         <div lang=fr><b>x</b></div><div lang=en-GB><b>y</b></div><a href=#><span>l</span></a><a><span>n</span></a>\
         <input type=checkbox checked><label>c</label><input type=checkbox><label>u</label>\
         <div id=t><span>t</span></div><div id=u><span>u</span></div><div class=on><span>o</span></div>\
         <div class='on off'><span>p</span></div>",
    );
}

#[test]
fn inline_styles_hints_custom_properties_and_nesting() {
    same_both_ways(
        "<!doctype html><style>:root { --c: red } .a { --c: blue; & > span { color: var(--c) } } \
         .b { .c & { margin: 1px } } span { border-color: var(--c) }</style>\
         <div class=a><span>1</span></div><div><span>2</span></div><div class=c><p class=b>x</p></div>\
         <p class=b>y</p><span style='color: green'>s</span><span style='color: green'>t</span>\
         <table width=50 bgcolor=red><tr><td align=center>c</td></tr></table><font size=5><span>f</span></font>",
    );
}

#[test]
fn ids_and_classes_no_selector_names_do_not_split_keys() {
    let items: String = (0..200)
        .map(|i| format!("<p id=p{i} class='x{i} k'>t</p>"))
        .collect();
    let rate = same_both_ways(&format!(
        "<!doctype html><style>.k {{ color: red }}</style>{items}"
    ));
    assert!(rate > 0.95, "{rate}");
}

#[test]
fn a_state_on_an_ancestor_splits_keys() {
    use madcawl_css::selector::PseudoClass;
    use madcawl_css::style::ElementStates;
    use madcawl_html::atom;
    let dom = parse(
        "<!doctype html><style>.p:hover span { color: red } li:nth-child(2) span { float: left } \
         [data-x='1'] span { display: block }</style><ul><li class=p><span>a</span></li>\
         <li class=p data-x=1><span>b</span></li><li class=p data-x=2><span>b</span></li></ul>",
    );
    let list = sheets(&dom);
    let mut c = cascade(&list);
    c.set_verify(true);
    let mut st = ElementStates::new();
    let li = dom
        .descendants(dom.document())
        .find(|&n| dom.local_name(n) == Some(atom!("li")))
        .expect("an item");
    st.set(li, PseudoClass::Hover, true);
    let keyed = c.compute_with(&dom, &st);
    assert_eq!(keyed.stats().mismatches, 0);
    c.set_path_key(false);
    assert_eq!(differences(&dom, &keyed, &c.compute_with(&dom, &st)), 0);
}
