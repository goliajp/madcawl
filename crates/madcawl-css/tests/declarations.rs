// a style attribute's declaration block, read and changed as CSSOM does
use madcawl_css::style::{Declarations, canonical, property_names};

fn value(d: &Declarations, name: &str) -> String {
    let mut s = String::new();
    d.value(name, &mut s);
    s
}

fn css(d: &Declarations) -> String {
    let mut s = String::new();
    d.write_css(&mut s);
    s
}

#[test]
fn longhands_read_back_as_specified() {
    let d = Declarations::parse(
        "DISPLAY: None; width: 10PX; height:1.50em; opacity: 50%; color: RED; top: 1in; z-index: 3",
    );
    assert_eq!(value(&d, "display"), "none");
    assert_eq!(value(&d, "width"), "10px");
    assert_eq!(value(&d, "height"), "1.5em");
    assert_eq!(value(&d, "opacity"), "50%");
    assert_eq!(value(&d, "color"), "red");
    assert_eq!(value(&d, "top"), "1in");
    assert_eq!(value(&d, "z-index"), "3");
    assert_eq!(value(&d, "visibility"), "");
    assert_eq!(d.len(), 7);
    assert_eq!(d.name(0), Some("display"));
    assert_eq!(
        css(&d),
        "display: none; width: 10px; height: 1.5em; opacity: 50%; color: red; top: 1in; z-index: 3;"
    );
}

#[test]
fn typed_values_serialize_canonically() {
    let d = Declarations::parse(
        "color: #FF000080; width: 0; margin-left: calc(10px + 5%); font-family: Arial, \"My Font\", serif; flex-basis: auto",
    );
    assert_eq!(value(&d, "color"), "rgba(255, 0, 0, 0.5)");
    assert_eq!(value(&d, "width"), "0px");
    assert_eq!(value(&d, "margin-left"), "calc(5% + 10px)");
    assert_eq!(value(&d, "font-family"), "Arial, \"My Font\", serif");
    assert_eq!(value(&d, "flex-basis"), "auto");
}

#[test]
fn unknown_and_invalid_declarations_are_dropped() {
    let d = Declarations::parse("bogus-prop: 1; width: red; display: block; display: bogus");
    assert_eq!(css(&d), "display: block;");
}

#[test]
fn a_later_declaration_replaces_an_earlier_unless_that_one_is_important() {
    let d = Declarations::parse("color: red; width: 1px; color: blue");
    assert_eq!(css(&d), "width: 1px; color: blue;");
    let d = Declarations::parse("color: red !important; color: blue");
    assert_eq!(css(&d), "color: red !important;");
    assert!(d.important("color"));
    assert!(!d.important("width"));
}

#[test]
fn set_keeps_the_place_and_refuses_what_is_not_a_value() {
    let mut d = Declarations::parse("color: red; width: 1px");
    assert!(d.set("color", "blue", false));
    assert_eq!(css(&d), "color: blue; width: 1px;");
    assert!(
        !d.set("color", "blue", false),
        "the same declaration changes nothing"
    );
    assert!(!d.set("width", "red", false));
    assert!(!d.set("width", "1px !important", false));
    assert!(!d.set("width", "1px; color: green", false));
    assert!(!d.set("bogus-prop", "1", false));
    assert!(d.set("height", "2px", true));
    assert_eq!(css(&d), "color: blue; width: 1px; height: 2px !important;");
    assert!(d.set("--Brand", " a  b ", false));
    assert_eq!(value(&d, "--Brand"), "a  b");
    assert_eq!(value(&d, "--brand"), "");
}

#[test]
fn shorthands_set_their_longhands_and_read_back_shortest() {
    let mut d = Declarations::new();
    assert!(d.set("margin", "1px 2px", false));
    assert_eq!(value(&d, "margin-top"), "1px");
    assert_eq!(value(&d, "margin-right"), "2px");
    assert_eq!(value(&d, "margin"), "1px 2px");
    assert_eq!(css(&d), "margin: 1px 2px;");
    assert!(d.set("margin-left", "3px", false));
    assert_eq!(value(&d, "margin"), "1px 2px 1px 3px");
    assert!(d.set("margin-top", "4px", true));
    assert_eq!(value(&d, "margin"), "", "longhands of two priorities");
    assert_eq!(
        css(&d),
        "margin-top: 4px !important; margin-right: 2px; margin-bottom: 1px; margin-left: 3px;"
    );
    assert!(d.remove("margin"));
    assert!(d.is_empty());
    assert!(!d.remove("margin"));
}

#[test]
fn border_and_flex_shorthands() {
    let d = Declarations::parse("border: 1px solid red");
    assert_eq!(value(&d, "border"), "1px solid red");
    assert_eq!(value(&d, "border-top"), "1px solid red");
    assert_eq!(value(&d, "border-width"), "1px");
    assert_eq!(css(&d), "border: 1px solid red;");
    let d = Declarations::parse("border: none; border-left-color: blue");
    assert_eq!(value(&d, "border"), "");
    assert_eq!(value(&d, "border-top"), "none");
    let d = Declarations::parse("flex: 1; overflow: hidden auto; gap: 2px");
    assert_eq!(value(&d, "flex"), "1 1 0%");
    assert_eq!(value(&d, "overflow"), "hidden auto");
    assert_eq!(value(&d, "gap"), "2px");
    assert_eq!(css(&d), "flex: 1 1 0%; overflow: hidden auto; gap: 2px;");
}

#[test]
fn var_in_a_shorthand_reads_back_as_written() {
    let d = Declarations::parse("padding: var(--p) 1px");
    assert_eq!(value(&d, "padding"), "var(--p) 1px");
    assert_eq!(value(&d, "padding-top"), "");
    assert_eq!(css(&d), "padding: var(--p) 1px;");
    let d = Declarations::parse("width: var(--w)");
    assert_eq!(value(&d, "width"), "var(--w)");
}

#[test]
fn css_wide_keywords_and_aliases() {
    let d = Declarations::parse("margin: inherit; -webkit-flex-direction: column");
    assert_eq!(value(&d, "margin"), "inherit");
    assert_eq!(value(&d, "flex-direction"), "column");
    assert_eq!(value(&d, "-webkit-flex-direction"), "column");
    assert_eq!(css(&d), "margin: inherit; flex-direction: column;");
}

#[test]
fn every_supported_name_resolves() {
    let names = property_names();
    for n in names {
        assert!(canonical(n).is_some(), "{n}");
    }
    let mut sorted = names.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "each name once");
    assert!(
        names.contains(&"display")
            && names.contains(&"margin-inline-start")
            && names.contains(&"grid-area")
    );
    assert_eq!(canonical("grid-gap"), Some("gap"));
    assert_eq!(canonical("cursor"), Some("cursor"));
    assert_eq!(canonical("bogus-prop"), None);
}

#[test]
fn properties_the_cascade_does_not_compute_are_kept_as_written() {
    let mut d = Declarations::parse(
        "Transform: scale(2) ; cursor: pointer; color: red; transition: opacity  1s !important",
    );
    assert!(d.set("display", "none", false));
    assert_eq!(
        css(&d),
        "transform: scale(2); cursor: pointer; color: red; transition: opacity  1s !important; display: none;"
    );
    assert_eq!(value(&d, "transform"), "scale(2)");
    assert!(d.important("transition"));
    assert!(d.set("cursor", "default", true));
    assert!(!d.set("cursor", "", false), "an empty value is no value");
    assert!(d.remove("transform"));
    assert_eq!(
        css(&d),
        "cursor: default !important; color: red; transition: opacity  1s !important; display: none;"
    );
}

#[test]
fn sums_sort_their_units_and_font_keeps_bold() {
    let d = Declarations::parse("width: calc(1px + 1em + 2%); font: bold 12px serif");
    assert_eq!(value(&d, "width"), "calc(2% + 1em + 1px)");
    assert_eq!(value(&d, "font-weight"), "bold");
    assert_eq!(value(&d, "font"), "bold 12px serif");
}

#[test]
fn a_value_of_an_uncomputed_property_is_one_value_or_none() {
    let mut d = Declarations::parse("color: red");
    for bad in [
        "scale(2); color: blue",
        "scale(2",
        "scale(2]",
        "}",
        "a{b}",
        "scale(2) !x",
        "\"open",
        "url(x",
    ] {
        assert!(!d.set("transform", bad, false), "{bad}");
    }
    assert_eq!(css(&d), "color: red;");
    assert!(d.set("transform", "translate(calc(1px + 2px)) [a]", false));
    assert!(d.set("-webkit-transition", "opacity 1s", false));
    assert_eq!(value(&d, "transition"), "opacity 1s");
    let d = Declarations::parse("transform: scale(2; color: red");
    assert_eq!(css(&d), "", "the parser took the rest as the open function");
    let d = Declarations::parse("color: red; transform: scale(2");
    assert_eq!(css(&d), "color: red;");
}

#[test]
fn a_custom_property_value_is_one_value_and_may_hold_blocks() {
    let mut d = Declarations::parse("color: red");
    for bad in ["a;b", "a(b", "a]", "{a", "a}", "a !b", "\"open", "url(x"] {
        assert!(!d.set("--x", bad, false), "{bad}");
    }
    assert_eq!(css(&d), "color: red;");
    assert!(d.set("--x", "{ a: b; c } [d] (e)", false));
    assert_eq!(value(&d, "--x"), "{ a: b; c } [d] (e)");
    let d = Declarations::parse("color: red; --z: a(b");
    assert_eq!(css(&d), "color: red;");
}

#[test]
fn a_comment_open_at_the_end_is_closed_there() {
    let d = Declarations::parse("cursor: pointer; transform: scale(2) /* x");
    assert_eq!(css(&d), "cursor: pointer; transform: scale(2);");
    let d = Declarations::parse("--y: 1 /* x");
    assert_eq!(value(&d, "--y"), "1");
}
