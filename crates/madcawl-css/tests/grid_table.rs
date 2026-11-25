//! The grid and table properties: their computed values as `getComputedStyle` writes them where it does
//! not give used track sizes (the expected strings are WebKit's), the shorthands, and what does not parse.

#[allow(dead_code)]
mod style;

use madcawl_css::style::Prop;
use madcawl_css::style::grid::{GridLine, TrackEntry};
use style::page;

#[track_caller]
fn check(p: &style::Page, cases: &[(&str, &str, &str)]) {
    for (id, prop, want) in cases {
        let got = p.style(id).css(Prop::from_name(prop).expect("a property"));
        assert_eq!(got, *want, "#{id} {prop}");
    }
}

#[test]
fn track_lists_areas_and_lines_serialize_as_webkit_writes_them() {
    let p = page(
        "<!doctype html><table id=t><tr><td id=c>x</td></tr></table><div id=d style='border-spacing:3px 4px;\
         grid-template-columns:[a] 1fr repeat(2, 10px [b]) minmax(10px,auto) fit-content(20%);\
         grid-template-areas:\"a b\" \"c d\";grid-row:2/span 3;grid-column:foo;grid-auto-flow:dense;\
         grid-auto-rows:10px auto'></div><div id=e style='grid-template-columns:repeat(auto-fill, \
         minmax(100px, 1fr)); grid-row: 3 foo / span 2 bar'></div>\
         <div id=g style='display:grid;grid-area:a'></div>",
    );
    check(
        &p,
        &[
            ("t", "border-spacing", "2px"),
            ("t", "border-collapse", "separate"),
            ("t", "caption-side", "top"),
            ("t", "table-layout", "auto"),
            ("t", "justify-items", "normal"),
            ("t", "justify-self", "auto"),
            ("t", "grid-template-columns", "none"),
            ("t", "grid-template-areas", "none"),
            ("t", "grid-auto-flow", "normal"),
            ("t", "grid-auto-rows", "auto"),
            ("c", "border-spacing", "2px"),
            ("c", "vertical-align", "middle"),
            ("d", "border-spacing", "3px 4px"),
            (
                "d",
                "grid-template-columns",
                "[a] 1fr repeat(2, 10px [b]) minmax(10px, auto) fit-content(20%)",
            ),
            ("d", "grid-template-areas", "\"a b\" \"c d\""),
            ("d", "grid-auto-flow", "dense"),
            ("d", "grid-auto-rows", "10px auto"),
            ("d", "grid-row-start", "2"),
            ("d", "grid-row-end", "span 3"),
            ("d", "grid-column-start", "foo"),
            ("d", "grid-column-end", "foo"),
            ("e", "border-spacing", "0px"),
            (
                "e",
                "grid-template-columns",
                "repeat(auto-fill, minmax(100px, 1fr))",
            ),
            ("e", "grid-row-start", "3 foo"),
            ("e", "grid-row-end", "span 2 bar"),
            ("g", "grid-auto-flow", "row"),
            ("g", "grid-row-start", "a"),
            ("g", "grid-column-end", "a"),
        ],
    );
}

#[test]
fn shorthands_set_their_longhands() {
    let p = page(
        "<!doctype html><div id=a style='grid-template: [top] \"h h\" 40px [mid] [mid2] \"s m\" 1fr / 100px auto'>\
         </div><div id=b style='grid: auto-flow dense 30px / 1fr 2fr'></div>\
         <div id=c style='grid: 50px / auto-flow 2em'></div><div id=d style='grid-area: 1 / 2 / span 3'></div>\
         <div id=e style='place-items: center end; place-self: start'></div>\
         <div id=f style='grid-template: 10px 20px / 30px'></div>",
    );
    check(
        &p,
        &[
            ("a", "grid-template-rows", "[top] 40px [mid mid2] 1fr"),
            ("a", "grid-template-columns", "100px auto"),
            ("a", "grid-template-areas", "\"h h\" \"s m\""),
            ("b", "grid-auto-flow", "dense"),
            ("b", "grid-auto-rows", "30px"),
            ("b", "grid-template-columns", "1fr 2fr"),
            ("b", "grid-template-rows", "none"),
            ("c", "grid-auto-flow", "column"),
            ("c", "grid-auto-columns", "32px"),
            ("c", "grid-template-rows", "50px"),
            ("d", "grid-row-start", "1"),
            ("d", "grid-column-start", "2"),
            ("d", "grid-row-end", "span 3"),
            ("d", "grid-column-end", "auto"),
            ("e", "align-items", "center"),
            ("e", "justify-items", "end"),
            ("e", "align-self", "start"),
            ("e", "justify-self", "start"),
            ("f", "grid-template-rows", "10px 20px"),
            ("f", "grid-template-columns", "30px"),
            ("f", "grid-template-areas", "none"),
        ],
    );
    let areas = p
        .style("a")
        .rare
        .grid_template_areas
        .clone()
        .expect("areas");
    let h = areas.named.iter().find(|(n, _)| &**n == "h").expect("h");
    assert_eq!(h.1, [0, 1, 0, 2], "h spans both columns of the first row");
}

#[test]
fn invalid_values_are_dropped() {
    let p = page(
        "<!doctype html><div id=a style='grid-template-areas: \"a b\" \"b b\"; grid-row-start: 0; \
         grid-template-columns: repeat(auto-fill, 1fr); grid-auto-flow: row column; border-spacing: 10%; \
         grid-column-start: span 0; grid-template-rows: repeat(1001, 1px); justify-items: auto'></div>\
         <div id=b style='grid-template-areas: \"a a\" \"a a\"; grid-row-start: -2 x; \
         grid-template-columns: repeat(3, [x] 1fr)'></div>",
    );
    let a = p.style("a");
    assert!(a.rare.grid_template_areas.is_none(), "b is not a rectangle");
    assert_eq!(a.rare.grid_lines[0], GridLine::Auto);
    assert_eq!(a.rare.grid_lines[1], GridLine::Auto);
    assert!(
        a.rare.grid_template_columns.is_none(),
        "an automatic repeat takes fixed sizes"
    );
    check(
        &p,
        &[
            ("a", "grid-auto-flow", "normal"),
            ("a", "grid-template-rows", "repeat(1001, 1px)"),
            ("a", "border-spacing", "0px"),
            ("a", "justify-items", "normal"),
            ("b", "grid-row-start", "-2 x"),
        ],
    );
    let b = p.style("b");
    assert_eq!(
        b.rare.grid_template_areas.as_ref().map(|x| x.named.len()),
        Some(1)
    );
    let cols = b.rare.grid_template_columns.clone().expect("columns");
    assert!(matches!(&cols.0[0], TrackEntry::Repeat(_, inner) if inner.len() == 2));
}

#[test]
fn table_attributes_map_to_spacing_padding_and_borders() {
    let p = page(
        "<!doctype html><table id=t cellspacing=5 cellpadding=7 border=2><tr><td id=c>x</td></tr></table>\
         <table id=u border><tr><th id=h>x</th></tr></table>",
    );
    check(
        &p,
        &[
            ("t", "border-spacing", "5px"),
            ("t", "border-top-width", "2px"),
            ("t", "border-top-style", "outset"),
            ("c", "padding-left", "7px"),
            ("c", "border-left-width", "1px"),
            ("c", "border-left-style", "inset"),
            ("u", "border-top-width", "1px"),
            ("h", "padding-top", "1px"),
        ],
    );
}
