//! Stylesheets shared through a `SheetCache`: pages that link the same
//! text parse it once, and every page styles as it does alone.

#[allow(dead_code)]
mod style;

use std::borrow::Cow;

use madcawl_css::style::{
    Device, Fetched, LinkedSheets, Scope, SheetCache, style_document, style_document_cached,
    style_document_shared,
};
use madcawl_html::Encoding;
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

/// Linked sheets from a table of files, each named by a version that a
/// change of its bytes moves, as a file's identity does.
#[derive(Default)]
struct Files {
    files: Vec<(&'static str, Vec<u8>, u32)>,
    named: bool,
    fetches: u32,
}

impl Files {
    fn write(&mut self, href: &'static str, bytes: &[u8]) {
        match self.files.iter_mut().find(|f| f.0 == href) {
            Some(f) => {
                f.1 = bytes.to_vec();
                f.2 += 1;
            }
            None => self.files.push((href, bytes.to_vec(), 0)),
        }
    }

    fn name(&self, href: &str) -> Option<Vec<u8>> {
        let f = self.files.iter().find(|f| f.0 == href)?;
        self.named.then(|| format!("{href}@{}", f.2).into_bytes())
    }
}

impl LinkedSheets for Files {
    fn identity(&mut self, href: &str) -> Option<Vec<u8>> {
        self.name(href)
    }

    fn fetch(&mut self, href: &str) -> Option<Fetched<'_>> {
        self.fetches += 1;
        let identity = self.name(href);
        let f = self.files.iter().find(|f| f.0 == href)?;
        Some(Fetched {
            bytes: Cow::Borrowed(&f.1),
            identity,
        })
    }
}

fn site_files(named: bool) -> Files {
    let mut f = Files {
        named,
        ..Files::default()
    };
    f.write("site.css", SITE.as_bytes());
    f.write("other.css", b".x { color: red }");
    f
}

#[test]
fn a_named_sheet_held_is_not_fetched_again() {
    let cache = SheetCache::new(1 << 20);
    let mut files = site_files(true);
    let device = Device::default();
    for round in 0..2 {
        for html in pages() {
            let dom = parse(html);
            let alone = style_document(&dom, device, &mut load);
            let cached =
                style_document_cached(&dom, device, Encoding::Utf8, &mut files, &cache, SCOPE);
            assert_eq!(differences(&dom, &alone, &cached), 0, "{html}");
        }
        // site.css and other.css read in the first round only; the third
        // page's site.css does not apply, so it is never read
        assert_eq!(files.fetches, 2, "round {round}");
    }
    // the second page's <style> is found by its text, as before
    let s = cache.stats();
    assert_eq!((s.parses, s.unread, s.hits), (3, 4, 1), "{s:?}");
}

#[test]
fn a_sheet_whose_name_moved_is_read_again() {
    let cache = SheetCache::new(1 << 20);
    let mut files = site_files(true);
    let html = r#"<!doctype html><link rel=stylesheet href=other.css><p class=x id=p>d</p>"#;
    let dom = parse(html);
    let red = style_document_cached(
        &dom,
        Device::default(),
        Encoding::Utf8,
        &mut files,
        &cache,
        SCOPE,
    );
    files.write("other.css", b".x { color: blue }");
    let blue = style_document_cached(
        &dom,
        Device::default(),
        Encoding::Utf8,
        &mut files,
        &cache,
        SCOPE,
    );
    assert_eq!(files.fetches, 2);
    let want = style_document(&dom, Device::default(), &mut |_| {
        Some(".x { color: blue }".into())
    });
    assert_eq!(differences(&dom, &want, &blue), 0);
    assert!(differences(&dom, &red, &blue) > 0);
    // the same bytes again under a new name: read, then found by text
    files.write("other.css", b".x { color: blue }");
    style_document_cached(
        &dom,
        Device::default(),
        Encoding::Utf8,
        &mut files,
        &cache,
        SCOPE,
    );
    let s = cache.stats();
    assert_eq!(
        (files.fetches, s.parses, s.hits, s.unread),
        (3, 2, 1, 0),
        "{s:?}"
    );
}

#[test]
fn an_unnamed_sheet_is_read_every_time_and_parsed_once() {
    let cache = SheetCache::new(1 << 20);
    let mut files = site_files(false);
    let dom = parse(pages()[0]);
    for _ in 0..3 {
        let cached = style_document_cached(
            &dom,
            Device::default(),
            Encoding::Utf8,
            &mut files,
            &cache,
            SCOPE,
        );
        assert_eq!(
            differences(
                &dom,
                &style_document(&dom, Device::default(), &mut load),
                &cached
            ),
            0
        );
    }
    let s = cache.stats();
    assert_eq!(
        (files.fetches, s.parses, s.hits, s.unread),
        (3, 1, 2, 0),
        "{s:?}"
    );
}

#[test]
fn a_name_holds_for_one_fallback_encoding() {
    // windows-1252 and UTF-8 read the byte E9 differently
    let cache = SheetCache::new(1 << 20);
    let mut files = Files {
        named: true,
        ..Files::default()
    };
    files.write("a.css", b"p { font-family: \"\xe9\" }");
    let dom = parse(r#"<!doctype html><link rel=stylesheet href=a.css><p>x</p>"#);
    let latin = style_document_cached(
        &dom,
        Device::default(),
        Encoding::Windows1252,
        &mut files,
        &cache,
        SCOPE,
    );
    let utf8 = style_document_cached(
        &dom,
        Device::default(),
        Encoding::Utf8,
        &mut files,
        &cache,
        SCOPE,
    );
    assert_eq!(files.fetches, 2);
    assert!(differences(&dom, &latin, &utf8) > 0);
}
