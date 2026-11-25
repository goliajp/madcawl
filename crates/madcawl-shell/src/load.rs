//! A local page to a styled DOM: the bytes in whatever encoding they
//! are, the stylesheets its `<link>`s name read from files beside it.

use std::io::Read as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

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

/// The largest file a page or a stylesheet is read from.
pub const MAX_FILE: u64 = 64 << 20;

#[cfg(target_os = "macos")]
const O_NONBLOCK: i32 = 0x4;
#[cfg(not(target_os = "macos"))]
const O_NONBLOCK: i32 = 0o4000;

/// The bytes of a regular file of at most `max` bytes. A device such as
/// `/dev/zero` or a FIFO is refused: the open does not block on a FIFO,
/// and what was opened is checked again, so a swap after the first
/// check is caught too.
pub fn read_file(path: &Path, max: u64) -> std::io::Result<Vec<u8>> {
    let not_file = || std::io::Error::other("not a regular file");
    if !std::fs::metadata(path)?.is_file() {
        return Err(not_file());
    }
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(path)?;
    if !f.metadata()?.is_file() {
        return Err(not_file());
    }
    let mut out = Vec::new();
    f.take(max + 1).read_to_end(&mut out)?;
    if out.len() as u64 > max {
        return Err(std::io::Error::other(format!(
            "the file is more than the {max} bytes one may be"
        )));
    }
    Ok(out)
}

/// The stylesheet a `<link>` names, when it is a regular file under one
/// of `allow` (any file when `allow` is None).
pub fn sheet_file(base: &Path, href: &str, allow: Option<&[PathBuf]>) -> Option<Vec<u8>> {
    let mut p = local(base, href)?;
    if let Some(allow) = allow {
        // read the checked path itself, so a link swapped in between is not followed
        p = p.canonicalize().ok()?;
        if !allow.iter().any(|a| p.starts_with(a)) {
            return None;
        }
    }
    read_file(&p, MAX_FILE).ok()
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
    style_within(dom, base, encoding, sheets, device, None)
}

/// The same with stylesheets read only from below the `allow`
/// directories.
pub fn style_within(
    dom: &Dom,
    base: Option<&Path>,
    encoding: Encoding,
    sheets: &mut Sheets,
    device: Device,
    allow: Option<&[PathBuf]>,
) -> Styles {
    style_document(dom, device, &mut |href| {
        let text = base
            .and_then(|b| sheet_file(b, href, allow))
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
