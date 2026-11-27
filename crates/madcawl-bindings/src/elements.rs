//! Which interface an element's wrapper has: HTML's "element interface"
//! for a name in the HTML namespace, narrowed to the interfaces bound.

use madcawl_dom::{Dom, NodeId};
use madcawl_html::Namespace;
use madcawl_js::{ClassDef, Engine};
use madcawl_str::{Atom, STATIC_ATOMS, atom};

use crate::generated::{
    ElementClass, HTMLAnchorElementClass, HTMLBodyElementClass, HTMLButtonElementClass, HTMLDivElementClass,
    HTMLElementClass, HTMLHeadElementClass, HTMLHtmlElementClass, HTMLIFrameElementClass, HTMLImageElementClass,
    HTMLInputElementClass, HTMLLinkElementClass, HTMLObjectElementClass, HTMLScriptElementClass, HTMLSpanElementClass,
    HTMLStyleElementClass, HTMLUnknownElementClass,
};
use crate::host::DomHost;

type Class<E> = &'static ClassDef<E, DomHost<E>>;

/// The names the standard defines an element for whose interface is not
/// bound yet, or is HTMLElement itself: they get HTMLElement.
const OTHERS: [Atom; 109] = [
    atom!("abbr"), atom!("address"), atom!("area"), atom!("article"), atom!("aside"), atom!("audio"), atom!("b"),
    atom!("base"), atom!("bdi"), atom!("bdo"), atom!("blockquote"), atom!("br"), atom!("canvas"), atom!("caption"),
    atom!("cite"), atom!("code"), atom!("col"), atom!("colgroup"), atom!("data"), atom!("datalist"), atom!("dd"),
    atom!("del"), atom!("details"), atom!("dfn"), atom!("dialog"), atom!("dl"), atom!("dt"), atom!("em"),
    atom!("embed"), atom!("fieldset"), atom!("figcaption"), atom!("figure"), atom!("footer"), atom!("form"),
    atom!("h1"), atom!("h2"), atom!("h3"), atom!("h4"), atom!("h5"), atom!("h6"), atom!("header"), atom!("hgroup"),
    atom!("hr"), atom!("i"), atom!("ins"), atom!("kbd"), atom!("label"), atom!("legend"), atom!("li"), atom!("main"),
    atom!("map"), atom!("mark"), atom!("menu"), atom!("meta"), atom!("meter"), atom!("nav"), atom!("noscript"),
    atom!("ol"), atom!("optgroup"), atom!("option"), atom!("output"), atom!("p"), atom!("param"), atom!("picture"),
    atom!("pre"), atom!("progress"), atom!("q"), atom!("rp"), atom!("rt"), atom!("ruby"), atom!("s"), atom!("samp"),
    atom!("search"), atom!("section"), atom!("select"), atom!("selectedcontent"), atom!("slot"), atom!("small"),
    atom!("source"), atom!("strong"), atom!("sub"), atom!("summary"), atom!("sup"), atom!("table"), atom!("tbody"),
    atom!("td"), atom!("template"), atom!("textarea"), atom!("tfoot"), atom!("th"), atom!("thead"), atom!("time"),
    atom!("title"), atom!("tr"), atom!("track"), atom!("u"), atom!("ul"), atom!("var"), atom!("video"),
    atom!("wbr"),
    // obsolete, with interfaces of their own or HTMLElement
    atom!("frame"), atom!("frameset"), atom!("marquee"), atom!("font"), atom!("dir"), atom!("listing"),
    atom!("xmp"),
    // obsolete, defined as HTMLElement
    atom!("acronym"), atom!("basefont"),
];

/// The rest of the obsolete names defined as HTMLElement.
const OBSOLETE: [Atom; 10] = [
    atom!("big"), atom!("center"), atom!("nobr"), atom!("noembed"), atom!("noframes"), atom!("plaintext"),
    atom!("rb"), atom!("rtc"), atom!("strike"), atom!("tt"),
];

/// Names that are not valid custom element names though they match the
/// production.
const RESERVED: [&[u8]; 8] = [
    b"annotation-xml", b"color-profile", b"font-face", b"font-face-src", b"font-face-uri", b"font-face-format",
    b"font-face-name", b"missing-glyph",
];

/// The names whose interfaces are bound, in the order `bound` gives
/// their classes.
const BOUND: [Atom; 14] = [
    atom!("div"), atom!("span"), atom!("a"), atom!("img"), atom!("script"), atom!("link"), atom!("style"),
    atom!("input"), atom!("button"), atom!("iframe"), atom!("object"), atom!("body"), atom!("head"), atom!("html"),
];

fn bound<E: Engine>(i: u8) -> Class<E> {
    match i {
        0 => &HTMLDivElementClass::<E>::DEF,
        1 => &HTMLSpanElementClass::<E>::DEF,
        2 => &HTMLAnchorElementClass::<E>::DEF,
        3 => &HTMLImageElementClass::<E>::DEF,
        4 => &HTMLScriptElementClass::<E>::DEF,
        5 => &HTMLLinkElementClass::<E>::DEF,
        6 => &HTMLStyleElementClass::<E>::DEF,
        7 => &HTMLInputElementClass::<E>::DEF,
        8 => &HTMLButtonElementClass::<E>::DEF,
        9 => &HTMLIFrameElementClass::<E>::DEF,
        10 => &HTMLObjectElementClass::<E>::DEF,
        11 => &HTMLBodyElementClass::<E>::DEF,
        12 => &HTMLHeadElementClass::<E>::DEF,
        _ => &HTMLHtmlElementClass::<E>::DEF,
    }
}

/// A static name the standard defines no element for.
const UNDEFINED: u8 = 0;
/// A static name whose element gets HTMLElement.
const PLAIN: u8 = 1;

/// What each static atom names as an HTML element: `UNDEFINED`, `PLAIN`,
/// or 2 plus its index in `BOUND`.
static KINDS: [u8; STATIC_ATOMS] = kinds();

const fn kinds() -> [u8; STATIC_ATOMS] {
    let mut t = [UNDEFINED; STATIC_ATOMS];
    let mut i = 0;
    while i < OTHERS.len() {
        t[OTHERS[i].index() as usize] = PLAIN;
        i += 1;
    }
    i = 0;
    while i < OBSOLETE.len() {
        t[OBSOLETE[i].index() as usize] = PLAIN;
        i += 1;
    }
    i = 0;
    while i < BOUND.len() {
        t[BOUND[i].index() as usize] = 2 + i as u8;
        i += 1;
    }
    t
}

/// A valid custom element name: a lower case ASCII letter, then the
/// characters of the PCENChar production with at least one hyphen, and
/// none of the reserved names.
fn is_custom_name(name: &[u8]) -> bool {
    let Ok(s) = std::str::from_utf8(name) else { return false };
    let pcen = |c: char| {
        matches!(c, '-' | '.' | '0'..='9' | '_' | 'a'..='z' | '\u{B7}' | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}'
            | '\u{F8}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}' | '\u{203F}'..='\u{2040}'
            | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}'
            | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
    };
    name.first().is_some_and(u8::is_ascii_lowercase)
        && name.contains(&b'-')
        && s.chars().all(pcen)
        && !RESERVED.contains(&name)
}

/// The class of element `el`'s wrapper.
pub(crate) fn class_of<E: Engine>(dom: &Dom, el: NodeId) -> Class<E> {
    if dom.namespace(el) != Some(Namespace::Html) {
        return &ElementClass::<E>::DEF;
    }
    let local = dom.local_name(el).expect("an element");
    let kind = if local.is_static() { KINDS[local.index() as usize] } else { UNDEFINED };
    match kind {
        UNDEFINED if local.with_name(is_custom_name) => &HTMLElementClass::<E>::DEF,
        UNDEFINED => &HTMLUnknownElementClass::<E>::DEF,
        PLAIN => &HTMLElementClass::<E>::DEF,
        k => bound::<E>(k - 2),
    }
}

#[cfg(test)]
mod tests {
    use super::is_custom_name;

    #[test]
    fn custom_element_names_follow_the_production() {
        for ok in ["my-el", "a-", "x-\u{e9}t\u{e9}", "a.b-c_1"] {
            assert!(is_custom_name(ok.as_bytes()), "{ok}");
        }
        for no in ["myel", "My-el", "-a", "1-a", "a-B", "font-face", "missing-glyph", "a-b c", "a-\u{2000}"] {
            assert!(!is_custom_name(no.as_bytes()), "{no}");
        }
    }
}
