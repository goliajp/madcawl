//! When the tree builder asks the sink to associate an element with the
//! form element pointer (HTML Living Standard 13.2.6.1, "create an
//! element for a token"), and when it does not. Which elements are
//! form-associated and which of those are listed: 4.10.2.

use madcawl_html::{
    Attrs, FragmentContext, Namespace, Options, QuirksMode, Step, Tokenizer, TreeBuilder, TreeSink,
};

#[allow(dead_code)]
#[path = "support/dom.rs"]
mod dom;

use dom::Dom;

fn names(dom: &Dom) -> Vec<String> {
    dom.forms
        .iter()
        .map(|&(el, form, same)| {
            assert_eq!(dom.name(form), b"form");
            assert!(same, "the parser only builds one tree");
            String::from_utf8_lossy(dom.name(el)).into_owned()
        })
        .collect()
}

fn document(html: &str) -> Vec<String> {
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(Dom::new(), Options::default());
    tok.feed(html.as_bytes());
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    names(&b.into_sink())
}

fn fragment(context: &[u8], html: &str) -> Vec<String> {
    let mut dom = Dom::new();
    let form = dom.create_element(Namespace::Html, b"form", Attrs::empty());
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new_fragment(
        dom,
        Options::default(),
        FragmentContext {
            ns: Namespace::Html,
            local: context,
            attrs: &[],
            form: Some(form),
            quirks: QuirksMode::NoQuirks,
        },
    );
    tok.set_state(b.tokenizer_state());
    tok.feed(html.as_bytes());
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    names(&b.into_sink())
}

#[test]
fn every_form_associated_element_is_associated() {
    assert_eq!(
        document(
            "<form><button></button><fieldset></fieldset><input><object></object>\
             <output></output><select></select><textarea></textarea><img>"
        ),
        [
            "button", "fieldset", "input", "object", "output", "select", "textarea", "img"
        ]
    );
}

#[test]
fn label_is_not_form_associated() {
    assert!(document("<form><label>x</label></form>").is_empty());
}

#[test]
fn other_elements_are_not_associated() {
    assert!(document("<form><div><p><span></span><a></a><option></option></form>").is_empty());
}

#[test]
fn a_listed_element_with_a_form_attribute_is_left_to_it() {
    assert_eq!(
        document(
            "<form><input form=f><button form=f></button><select form=f></select>\
             <textarea form=f></textarea><fieldset form=f></fieldset><output form=f></output>\
             <object form=f></object><img form=f><input>"
        ),
        ["img", "input"]
    );
}

#[test]
fn nested_inside_the_form() {
    assert_eq!(
        document("<form><div><table><tr><td><input></td></tr></table></div></form>"),
        ["input"]
    );
}

#[test]
fn nothing_after_the_form_is_closed() {
    assert_eq!(document("<form><input></form><input>"), ["input"]);
}

#[test]
fn the_pointer_outlives_an_unclosed_form_in_a_table() {
    // the form in table text is popped at once, the pointer stays set
    assert_eq!(
        document("<table><form><input type=hidden><tr><td><input>"),
        ["input", "input"]
    );
}

#[test]
fn not_in_template_contents() {
    assert!(document("<form><template><input><button></button></template></form>").is_empty());
}

#[test]
fn a_template_inside_the_form_stops_only_its_contents() {
    assert_eq!(
        document("<form><input><template><input></template><input></form>"),
        ["input", "input"]
    );
}

#[test]
fn not_in_a_fragment() {
    // the context's form is in another tree than the fragment's root
    assert!(fragment(b"div", "<input><button></button><img>").is_empty());
    assert!(fragment(b"form", "<input>").is_empty());
}

#[test]
fn not_in_foreign_content() {
    assert!(document("<form><svg><input></input><button></button></svg></form>").is_empty());
    // img breaks out of foreign content and is created as an HTML element
    assert_eq!(
        document("<form><svg><input></input><img></svg></form>"),
        ["img"]
    );
}
