//! A local page to a styled DOM: the bytes in whatever encoding they
//! are, the stylesheets its `<link>`s name read from files beside it.

use std::path::Path;

use madcawl_css::decode_stylesheet;
use madcawl_css::style::{Device, SheetSource, Styles, sheet_sources, style_document};
use madcawl_dom::Dom;
use madcawl_html::{DocumentParser, Encoding, Options, Source, Step};

/// What happened to a page's stylesheets.
#[derive(Default, Debug)]
pub struct Sheets {
    pub inline: usize,
    pub linked: usize,
    /// `<link>`ed sheets with no local file: remote, or not saved
    pub missing: Vec<String>,
}

/// `transport` is the charset a `Content-Type` would have given; a
/// local file has none unless the caller knows it.
pub fn parse(bytes: &[u8], transport: Option<Encoding>) -> (Dom, Encoding) {
    let source = Source {
        transport,
        ..Source::default()
    };
    let mut p = DocumentParser::new(Dom::new, Options::default(), source);
    p.feed(bytes);
    p.end();
    assert_eq!(p.run(), Step::Done, "all input was given");
    let e = p.encoding();
    (p.into_sink(), e)
}

/// The path a stylesheet `href` names below `base`, when it is a
/// relative reference to a file.
fn local(base: &Path, href: &str) -> Option<std::path::PathBuf> {
    let href = href.trim();
    let path = href.split(['?', '#']).next()?;
    let remote = path.contains(':') || path.starts_with('/') || path.is_empty();
    (!remote).then(|| base.join(path))
}

pub fn style(dom: &Dom, base: Option<&Path>, encoding: Encoding, sheets: &mut Sheets) -> Styles {
    style_for(dom, base, encoding, sheets, Device::default())
}

/// The same for another device, such as a screen of another scale.
pub fn style_for(
    dom: &Dom,
    base: Option<&Path>,
    encoding: Encoding,
    sheets: &mut Sheets,
    device: Device,
) -> Styles {
    style_document(dom, device, &mut |href| {
        let text = base
            .and_then(|b| local(b, href))
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| decode_stylesheet(&b, None, Some(encoding)).0);
        match &text {
            Some(_) => sheets.linked += 1,
            None => sheets.missing.push(href.to_owned()),
        }
        text
    })
}

/// How many `<style>` sheets the page has.
pub fn count_inline(dom: &Dom) -> usize {
    sheet_sources(dom)
        .iter()
        .filter(|s| matches!(s, SheetSource::Inline { .. }))
        .count()
}
