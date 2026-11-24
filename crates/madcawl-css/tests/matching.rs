//! Selector matching against the DOM, seen through `display`.

#[allow(dead_code)]
mod style;

use madcawl_css::style::Display;
use style::page;

/// The ids of the elements `selector` makes `display: none` in `body`.
fn hit(selector: &str, body: &str) -> Vec<String> {
    let p = page(&format!(
        "<style>{selector} {{ display: none }}</style>{body}"
    ));
    let mut out = Vec::new();
    for n in p.dom.descendants(p.dom.document()) {
        let (Some(s), Some(id)) = (
            p.styles.get(n),
            p.dom.attribute(n, madcawl_html::atom!("id")),
        ) else {
            continue;
        };
        if s.display == Display::None {
            out.push(id.as_ascii_str().unwrap().to_owned());
        }
    }
    out
}

#[test]
fn combinators() {
    let body = "<div id=a class=x><p id=b><span id=c></span></p><i id=d></i><b id=e></b></div>";
    assert_eq!(hit(".x span", body), ["c"]);
    assert_eq!(hit(".x > span", body), Vec::<String>::new());
    assert_eq!(hit(".x > p > span", body), ["c"]);
    assert_eq!(hit("p + i", body), ["d"]);
    assert_eq!(hit("p ~ b", body), ["e"]);
    assert_eq!(hit("p + b", body), Vec::<String>::new());
    assert_eq!(hit("div *", body), ["b", "c", "d", "e"]);
}

#[test]
fn attributes() {
    let body = "<a id=a href='https://x.test/a.pdf' lang=en-US rel='nofollow noopener' \
                type=TEXT data-k=v></a>";
    for s in [
        "[href]",
        "[href$='.pdf']",
        "[href^=https]",
        "[href*='x.test']",
        "[rel~=noopener]",
        "[lang|=en]",
        "[type=text]",
        "[data-k=V i]",
        "[HREF]",
    ] {
        assert_eq!(hit(s, body), ["a"], "{s}");
    }
    for s in [
        "[rel~=no]",
        "[lang|=e]",
        "[data-k=V]",
        "[type=text s]",
        "[href$='']",
    ] {
        assert!(hit(s, body).is_empty(), "{s}");
    }
}

#[test]
fn structural() {
    let body = "<ul><li id=a></li><li id=b></li><li id=c></li><li id=d></li></ul>\
                <div><p id=e></p><span id=f></span><p id=g></p></div><div id=h></div>\
                <div id=i> </div>";
    assert_eq!(hit("li:nth-child(2n)", body), ["b", "d"]);
    assert_eq!(hit("li:nth-last-child(1)", body), ["d"]);
    assert_eq!(hit("li:first-child, li:last-child", body), ["a", "d"]);
    assert_eq!(hit("p:first-of-type", body), ["e"]);
    assert_eq!(hit("p:last-of-type", body), ["g"]);
    assert_eq!(hit("span:only-of-type", body), ["f"]);
    assert_eq!(hit("div:empty", body), ["h"]);
    assert_eq!(hit(":nth-child(2 of p)", body), ["g"]);
}

#[test]
fn logical_and_has() {
    let body = "<div id=a><p class=x></p></div><div id=b><p></p><span></span></div>\
                <div id=c></div><p id=d class=y></p><i id=e></i>";
    assert_eq!(hit("div:has(.x)", body), ["a"]);
    assert_eq!(hit("div:has(> p + span)", body), ["b"]);
    assert_eq!(hit("p:has(+ i)", body), ["d"]);
    assert_eq!(hit("div:not(:has(*))", body), ["c"]);
    assert_eq!(hit(":is(#d, #e)", body), ["d", "e"]);
    assert_eq!(hit(":where(.y)", body), ["d"]);
}

#[test]
fn states() {
    let body = "<a id=a href=#></a><a id=b></a><input id=c type=checkbox checked>\
                <input id=d disabled><fieldset disabled><legend><input id=e></legend>\
                <input id=f></fieldset><input id=g placeholder=x><input id=h required>\
                <details id=i open></details><x-y id=j></x-y><p id=k lang=fr-CA></p>\
                <p id=l dir=rtl></p>";
    assert_eq!(hit(":link", body), ["a"]);
    assert_eq!(hit(":checked", body), ["c"]);
    assert_eq!(hit("input:disabled", body), ["d", "f"]);
    assert_eq!(hit(":placeholder-shown", body), ["g"]);
    assert_eq!(hit("input:invalid", body), ["h"]);
    assert_eq!(hit(":open", body), ["i"]);
    assert_eq!(hit(":not(:defined)", body), ["j"]);
    assert_eq!(hit("p:lang(fr)", body), ["k"]);
    assert_eq!(hit("p:dir(rtl)", body), ["l"]);
    assert!(hit("a:hover, a:visited, ::before, a::after", body).is_empty());
}

#[test]
fn quirks_mode_ids_and_classes_ignore_case() {
    let p = page(
        "<style>.Foo { display: none } #BAR { display: none }</style>\
                  <p id=a class=foo></p><p id=bar></p>",
    );
    assert_eq!(p.display("a"), Display::None);
    assert_eq!(p.display("bar"), Display::None);
    let p = page("<!doctype html><style>.Foo { display: none }</style><p id=a class=foo></p>");
    assert_eq!(p.display("a"), Display::Block);
}

#[test]
fn namespaces() {
    let body = "<svg><a id=s></a></svg><a id=h></a>";
    assert_eq!(hit("a", body), ["s", "h"]);
    assert_eq!(hit("|a, *|b", body), Vec::<String>::new());
    let p = page(&format!(
        "<style>@namespace svg url(http://www.w3.org/2000/svg); svg|a {{ display: none }}</style>{body}"
    ));
    assert_eq!(p.display("s"), Display::None);
    assert_eq!(p.display("h"), Display::Inline);
}
