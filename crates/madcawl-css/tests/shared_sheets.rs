//! Stylesheets shared through a `SheetCache`: pages that link the same
//! text parse it once, and every page styles as it does alone.

#[allow(dead_code)]
mod style;

use madcawl_css::style::{Device, Scope, SheetCache, style_document, style_document_shared};
use style::{differences, parse};

const SITE: &str = "
    :root { --gap: 3px }
    .card { margin: var(--gap); padding: 1em; color: rgb(1 2 3) }
    .card > h2 { font-size: 20px }
    @media (min-width: 1000px) { .wide { display: none } }
    @layer base { p { line-height: 1.5 } }
    .card { & .title { font-weight: 700 } }
    li:nth-child(2n) { float: left }
";

const SCOPE: Scope = Scope {
    partition: 1,
    site: "https://example.com",
};

fn load(href: &str) -> Option<String> {
    match href {
        "site.css" => Some(SITE.to_owned()),
        "other.css" => Some(".x { color: red }".to_owned()),
        _ => None,
    }
}

fn pages() -> [&'static str; 3] {
    [
        r#"<!doctype html><link rel=stylesheet href=site.css>
           <div class="card wide"><h2 class=title>a</h2><p>b</p></div>
           <ul><li>1<li>2<li>3</ul>"#,
        r#"<!doctype html><link rel=stylesheet href=site.css>
           <style>.card { color: green }</style>
           <section class=card><h2 class=title>c</h2></section>"#,
        r#"<!doctype html><link rel=stylesheet href=other.css>
           <link rel=stylesheet href=site.css media="(max-width: 10px)">
           <p class=x>d</p>"#,
    ]
}

#[test]
fn a_sheet_two_pages_link_parses_once() {
    let cache = SheetCache::new(1 << 20);
    let device = Device::default();
    for html in &pages()[..2] {
        style_document_shared(&parse(html), device, &mut load, &cache, SCOPE);
    }
    let s = cache.stats();
    // site.css once, the second page's <style> once; site.css again is a hit
    assert_eq!((s.parses, s.hits), (2, 1), "{s:?}");
    style_document_shared(&parse(pages()[0]), device, &mut load, &cache, SCOPE);
    assert_eq!(cache.stats().parses, 2);
    assert_eq!(cache.stats().hits, 2);
}

#[test]
fn shared_sheets_style_as_unshared_ones() {
    let cache = SheetCache::new(1 << 20);
    for device in [
        Device::default(),
        // under 1000px wide, so `.wide` stays shown
        Device {
            width: 800.0,
            height: 600.0,
            ..Device::default()
        },
    ] {
        // twice round, so the second styles every page from sheets other pages parsed
        for _ in 0..2 {
            for html in pages() {
                let dom = parse(html);
                let alone = style_document(&dom, device, &mut load);
                let shared = style_document_shared(&dom, device, &mut load, &cache, SCOPE);
                assert_eq!(differences(&dom, &alone, &shared), 0, "{html}");
            }
        }
    }
    assert!(cache.stats().hits > 0);
    // the two devices take different `@media` branches
    let dom = parse(pages()[0]);
    let narrow = Device {
        width: 800.0,
        ..Device::default()
    };
    let wide = style_document(&dom, Device::default(), &mut load);
    assert!(differences(&dom, &wide, &style_document(&dom, narrow, &mut load)) > 0);
}

#[test]
fn a_full_cache_still_styles_every_page() {
    // room for nothing: every sheet is parsed and none kept
    let cache = SheetCache::new(16);
    for html in pages() {
        let dom = parse(html);
        let alone = style_document(&dom, Device::default(), &mut load);
        let shared = style_document_shared(&dom, Device::default(), &mut load, &cache, SCOPE);
        assert_eq!(differences(&dom, &alone, &shared), 0);
    }
    let s = cache.stats();
    assert_eq!((s.hits, s.entries, s.weight), (0, 0, 0));
}
