//! The stylesheets a document names: its `<style>` elements (HTML and
//! SVG) and its `<link rel=stylesheet>` elements, in tree order.

use madcawl_dom::{Dom, NodeKind};
use madcawl_html::Namespace;
use madcawl_str::atom;

use super::matching::text;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SheetSource {
    Inline {
        text: String,
        media: Option<String>,
    },
    /// The `href` as written
    Link {
        href: String,
        media: Option<String>,
    },
}

pub fn sheet_sources(dom: &Dom) -> Vec<SheetSource> {
    let mut out = Vec::new();
    for n in dom.descendants(dom.document()) {
        if dom.kind(n) != NodeKind::Element {
            continue;
        }
        let ns = dom.namespace(n);
        let name = dom.local_name(n);
        let attr = |a| dom.attribute(n, a).map(|v| text(v).into_owned());
        let media = attr(atom!("media"));
        let style =
            name == Some(atom!("style")) && matches!(ns, Some(Namespace::Html | Namespace::Svg));
        if style {
            let css_type = attr(atom!("type")).is_none_or(|t| {
                let t = t.trim();
                t.is_empty() || t.eq_ignore_ascii_case("text/css")
            });
            if !css_type {
                continue;
            }
            let mut s = String::new();
            for c in dom.children(n) {
                if let Some(t) = dom.text(c).filter(|_| dom.kind(c) == NodeKind::Text) {
                    s.push_str(&text(t));
                }
            }
            out.push(SheetSource::Inline { text: s, media });
        } else if name == Some(atom!("link")) && ns == Some(Namespace::Html) {
            let rel = attr(atom!("rel")).unwrap_or_default().to_ascii_lowercase();
            let words = || rel.split(|c: char| c.is_ascii_whitespace());
            let sheet = words().any(|w| w == "stylesheet") && !words().any(|w| w == "alternate");
            if let Some(href) = attr(atom!("href")).filter(|_| sheet) {
                out.push(SheetSource::Link { href, media });
            }
        }
    }
    out
}
