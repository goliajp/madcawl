//! One document of a page: its DOM, its styles, and where its nodes are
//! in the page's viewport. A new navigation makes a new one.

use std::sync::LazyLock;

use madcawl_agent::Geometry;
use madcawl_css::decode_stylesheet;
use madcawl_css::style::{Device, Scope, SheetCache, Styles, style_document_shared};
use madcawl_dom::Dom;
use madcawl_html::Encoding;
use madcawl_layout::Layout;
use madcawl_net::Url;
use madcawl_net::cookie::site_of;

use super::net::partition_of;
use crate::load;

/// The stylesheets the pages of the host have parsed, shared only by the
/// pages of one agent session whose documents are of one site. 64 MiB of weight
/// holds the sheets of four of the largest sites (12 to 16 MiB each) or
/// dozens of ordinary ones, and is a sixteenth of what 64 pages may take
/// in all
static SHEETS: LazyLock<SheetCache> = LazyLock::new(|| SheetCache::new(64 << 20));

/// The site a document at `url` is of, for which pages share stylesheets:
/// the scheme and registrable domain of a web URL; for any other, its
/// scheme (every local file the host may open is the user's own).
pub(crate) fn site(url: &str) -> String {
    match Url::parse(url) {
        Ok(u) if matches!(u.scheme(), "http" | "https") => {
            format!("{}://{}", u.scheme(), site_of(u.host_str().unwrap_or("")))
        }
        Ok(u) => format!("{}:", u.scheme()),
        Err(_) => String::new(),
    }
}

/// Drops the stylesheets of agent session `session`, which has ended.
pub(crate) fn forget_session(session: u32) {
    SHEETS.forget(partition_of(session));
}

pub struct Doc {
    pub dom: Dom,
    pub styles: Styles,
    pub geometry: Geometry,
}

/// A response's body as a DOM: HTML as it is; plain text, as a browser
/// shows it, in a `pre`.
pub fn parse(body: &[u8], content_type: &str, charset: Option<Encoding>) -> (Dom, Encoding) {
    let essence = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if essence != "text/plain" {
        return load::parse(body, charset);
    }
    let text = String::from_utf8_lossy(body);
    let escaped = text.replace('&', "&amp;").replace('<', "&lt;");
    load::parse(format!("<!doctype html><pre>{escaped}</pre>").as_bytes(), Some(Encoding::Utf8))
}

impl Doc {
    /// Styles and lays out `dom`, a document at `url` of agent session
    /// `session`, in a viewport of `size`; a linked stylesheet's text
    /// comes from `sheet`, by its `href` as written.
    pub fn new(
        dom: Dom,
        encoding: Encoding,
        size: (f32, f32),
        (session, url): (u32, &str),
        sheet: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
    ) -> Doc {
        let site = site(url);
        let scope = Scope { partition: partition_of(session), site: &site };
        let device = Device { width: size.0 as f64, height: size.1 as f64, ..Device::default() };
        let styles = style_document_shared(
            &dom,
            device,
            &mut |href| sheet(href).map(|b| decode_stylesheet(&b, None, Some(encoding)).0),
            &SHEETS,
            scope,
        );
        let geometry = lay_out(&dom, &styles, size);
        Doc { dom, styles, geometry }
    }
}

fn lay_out(dom: &Dom, styles: &Styles, (w, h): (f32, f32)) -> Geometry {
    let mut layout = Layout::new(dom, styles, w, h);
    Geometry::from_layout(&mut layout, styles, w, h)
}

#[cfg(test)]
mod tests {
    use super::site;

    #[test]
    fn pages_share_stylesheets_by_site() {
        assert_eq!(site("https://www.example.co.uk/a"), "https://example.co.uk");
        assert_eq!(site("https://docs.example.co.uk/b?c"), "https://example.co.uk");
        assert_eq!(site("http://www.example.co.uk/"), "http://example.co.uk");
        assert_eq!(site("https://127.0.0.1:8080/"), "https://127.0.0.1");
        assert_eq!(site("file:///tmp/a.html"), "file:");
        assert_eq!(site("about:blank"), "about:");
    }
}
