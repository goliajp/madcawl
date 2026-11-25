//! One document of a page: its DOM, its styles, and where its nodes are
//! in the page's viewport. A new navigation makes a new one.

use madcawl_agent::Geometry;
use madcawl_css::decode_stylesheet;
use madcawl_css::style::{Device, Styles, style_document};
use madcawl_dom::Dom;
use madcawl_html::Encoding;
use madcawl_layout::Layout;

use crate::load;

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
    /// Styles and lays out `dom` in a viewport of `size`; a linked
    /// stylesheet's text comes from `sheet`, by its `href` as written.
    pub fn new(
        dom: Dom,
        encoding: Encoding,
        size: (f32, f32),
        sheet: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
    ) -> Doc {
        let device = Device { width: size.0 as f64, height: size.1 as f64, ..Device::default() };
        let styles = style_document(&dom, device, &mut |href| {
            sheet(href).map(|b| decode_stylesheet(&b, None, Some(encoding)).0)
        });
        let geometry = lay_out(&dom, &styles, size);
        Doc { dom, styles, geometry }
    }
}

fn lay_out(dom: &Dom, styles: &Styles, (w, h): (f32, f32)) -> Geometry {
    let mut layout = Layout::new(dom, styles, w, h);
    Geometry::from_layout(&mut layout, styles, w, h)
}
