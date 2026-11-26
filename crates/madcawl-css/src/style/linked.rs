//! A document's styles from its stylesheets, inline and linked, the
//! linked ones parsed once through a `SheetCache` and found there before
//! their bytes are read when the loader can name them.

use std::borrow::Cow;
use std::sync::Arc;

use madcawl_dom::Dom;
use madcawl_html::Encoding;

use crate::Span;
use crate::bytes::decode_stylesheet_borrowed;
use crate::sheet::Stylesheet;

use super::{
    Cascade, Device, Origin, Scope, SheetCache, SheetSource, Styles, matches_media, sheet_sources,
    ua_sheet,
};

/// Where a page's linked stylesheets come from.
pub trait LinkedSheets {
    /// A name for the bytes `fetch(href)` would give now, told without
    /// reading them: two equal names stand for equal bytes. `None` when
    /// the loader cannot tell.
    fn identity(&mut self, href: &str) -> Option<Vec<u8>>;

    /// The bytes of the stylesheet `href` names.
    fn fetch(&mut self, href: &str) -> Option<Fetched<'_>>;
}

/// A linked stylesheet's bytes, and the name (`identity`) they were read
/// under when the read can vouch the name stands for these bytes.
pub struct Fetched<'a> {
    pub bytes: Cow<'a, [u8]>,
    pub identity: Option<Vec<u8>>,
}

/// The styles `style_document_shared` gives, a linked sheet's bytes
/// decoded with the document's `encoding` as the fallback, and a sheet
/// the cache holds under the name its loader gives found without
/// reading it.
pub fn style_document_cached(
    dom: &Dom,
    device: Device,
    encoding: Encoding,
    links: &mut dyn LinkedSheets,
    cache: &SheetCache,
    scope: Scope,
) -> Styles {
    style_with(dom, device, &mut |source, applies| {
        if !applies {
            return None;
        }
        match source {
            SheetSource::Inline { text, .. } => cache.get(scope, text).ok(),
            SheetSource::Link { href, .. } => {
                // the text depends on the fallback encoding as well as the bytes
                let name = |mut id: Vec<u8>| {
                    id.push(0);
                    id.extend_from_slice(encoding.name().as_bytes());
                    id
                };
                if let Some(s) = links
                    .identity(&href)
                    .and_then(|id| cache.get_known(scope, &name(id)))
                {
                    return Some(s);
                }
                let f = links.fetch(&href)?;
                let (text, _) = decode_stylesheet_borrowed(&f.bytes, None, Some(encoding));
                let id = f.identity.map(name);
                cache.get_as(scope, text, id.as_deref()).ok()
            }
        }
    })
}

/// The styles `style_document` gives, the stylesheets parsed once for
/// the pages of `scope` that have them and held in `cache`.
pub fn style_document_shared(
    dom: &Dom,
    device: Device,
    load: &mut dyn FnMut(&str) -> Option<String>,
    cache: &SheetCache,
    scope: Scope,
) -> Styles {
    style_parsed_by(dom, device, load, &mut |t| cache.get(scope, t).ok())
}

pub(super) fn style_parsed_by(
    dom: &Dom,
    device: Device,
    load: &mut dyn FnMut(&str) -> Option<String>,
    parse: &mut dyn FnMut(String) -> Option<Arc<Stylesheet>>,
) -> Styles {
    style_with(dom, device, &mut |source, applies| {
        let text = match source {
            SheetSource::Inline { text, .. } => Some(text),
            SheetSource::Link { href, .. } => load(&href),
        };
        text.filter(|_| applies).and_then(&mut *parse)
    })
}

/// The styles of `dom` from the sheet `sheet` gives for each of its
/// stylesheet sources, told whether the source's `media` matches.
fn style_with(
    dom: &Dom,
    device: Device,
    sheet: &mut dyn FnMut(SheetSource, bool) -> Option<Arc<Stylesheet>>,
) -> Styles {
    let mut sheets = Vec::new();
    for s in sheet_sources(dom) {
        let media = match &s {
            SheetSource::Inline { media, .. } | SheetSource::Link { media, .. } => media,
        };
        let applies = media
            .as_deref()
            .is_none_or(|m| matches_media(m, Span::new(0, m.len()), &device));
        if let Some(sheet) = sheet(s, applies) {
            sheets.push(sheet);
        }
    }
    let mut c = Cascade::new(device);
    c.add(ua_sheet(), Origin::UserAgent);
    for s in &sheets {
        c.add(s, Origin::Author);
    }
    c.compute(dom)
}
