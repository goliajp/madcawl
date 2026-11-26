//! One document of a page: its DOM, its styles, and where its nodes are
//! in the page's viewport, laid out only once something asks where they
//! are. A new navigation makes a new one.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use madcawl_agent::{Frame, Geometry, Inputs, kind, tag};
use madcawl_css::style::{
    Device, Fetched, LinkedSheets, Scope, SheetCache, SheetCacheStats, Styles,
    style_document_cached,
};
use madcawl_dom::Dom;
use madcawl_html::Encoding;
use madcawl_layout::Layout;
use madcawl_net::Url;
use madcawl_net::cookie::site_of;

use super::net::partition_of;
use crate::load;

/// the stylesheets the pages of the host have parsed, shared only by the
/// pages of one agent session whose documents are of one site, bounded by
/// `SHEET_CACHE_CAP` apart from what pages hold
static SHEETS: LazyLock<SheetCache> = LazyLock::new(|| SheetCache::new(SHEET_CACHE_CAP));

/// the shared stylesheet cache's own budget, in the weight it estimates
/// (five bytes for a byte of text), apart from the pages' own memory:
/// twelve real sites (news, wikis, docs, github, youtube) hold 36 MiB of
/// it at once, 124 sheets; the largest single sheet, 3.2 MB of text,
/// weighs 15 MiB and is kept only under a quarter of the cap, so 64 MiB
/// is the least power of two that keeps such a site warm; half of it
/// drops that sheet and every open of its site parses it again, for no
/// memory a page saves, since the parse it holds is made anyway
const SHEET_CACHE_CAP: usize = 64 << 20;

/// what the shared stylesheet cache holds now, for every session, and
/// the most it holds
pub(crate) fn sheet_stats() -> (SheetCacheStats, usize) {
    (SHEETS.stats(), SHEETS.max_weight())
}

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

/// Linked stylesheets a load already holds the bytes of, by `href`.
pub struct HeldSheets<'a>(pub &'a HashMap<String, Vec<u8>>);

impl LinkedSheets for HeldSheets<'_> {
    fn identity(&mut self, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn fetch(&mut self, href: &str) -> Option<Fetched<'_>> {
        let bytes = Cow::Borrowed(self.0.get(href)?.as_slice());
        Some(Fetched { bytes, identity: None })
    }
}

/// A view request whose answer needs geometry: one of the viewport.
pub fn asks_geometry(f: &Frame) -> bool {
    f.kind == kind::READ && f.get_num(tag::SCOPE) == Some(1)
}

pub struct Doc {
    pub dom: Dom,
    pub styles: Styles,
    /// None until the first question that needs geometry (`lay_out`)
    pub geometry: Option<Geometry>,
    size: (f32, f32),
    /// How many times the document was laid out: once at most
    pub(crate) layouts: u32,
    /// The semantic tree's sequences just before the layout and once it
    /// took in the geometry (`Page::diff_floor`)
    pub laid_out_at: Option<(u64, u64)>,
}

thread_local! {
    /// The time this thread spent laying documents out, in all
    static LAYOUT_TIME: std::cell::Cell<Duration> = const { std::cell::Cell::new(Duration::ZERO) };
}

/// The time the calling thread has spent laying documents out, so a
/// request can tell its own share (a connection is one thread).
pub fn layout_time() -> Duration {
    LAYOUT_TIME.get()
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
    /// Styles `dom`, a document at `url` of agent session `session`, for a
    /// viewport of `size`; a linked stylesheet comes from `links`, by its
    /// `href` as written. It is not laid out yet.
    pub fn new(
        dom: Dom,
        encoding: Encoding,
        size: (f32, f32),
        (session, url): (u32, &str),
        links: &mut dyn LinkedSheets,
    ) -> Doc {
        let site = site(url);
        let scope = Scope { partition: partition_of(session), site: &site };
        let device = Device { width: size.0 as f64, height: size.1 as f64, ..Device::default() };
        let styles = style_document_cached(&dom, device, encoding, links, &SHEETS, scope);
        Doc { dom, styles, geometry: None, size, layouts: 0, laid_out_at: None }
    }

    /// The DOM, and the styles and geometry so far for the agent side.
    pub fn parts(&mut self) -> (&mut Dom, Inputs<'_>) {
        (&mut self.dom, Inputs { styles: Some(&self.styles), geometry: self.geometry.as_ref() })
    }

    /// The geometry, laid out now if it is not yet; true when it was.
    pub fn lay_out(&mut self) -> (&mut Geometry, bool) {
        let fresh = self.geometry.is_none();
        if fresh {
            let started = Instant::now();
            let (w, h) = self.size;
            let mut layout = Layout::new(&self.dom, &self.styles, w, h);
            self.geometry = Some(Geometry::from_layout(&mut layout, &self.styles, w, h));
            self.layouts += 1;
            LAYOUT_TIME.set(LAYOUT_TIME.get() + started.elapsed());
        }
        (self.geometry.as_mut().expect("laid out"), fresh)
    }
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
