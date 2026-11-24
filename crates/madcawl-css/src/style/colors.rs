//! Declared colors: `currentcolor`, the system colors, and the colors
//! of CSS Color, kept as legacy sRGB bytes when they are sRGB.

use crate::color::{ColorValue, Space, Syntax, parse_color};

use super::props::SpecColor;
use super::vals::Item;

/// The system colors as WebKit draws them in light mode on macOS.
fn system_color(w: &str) -> Option<[u8; 4]> {
    Some(match w {
        "canvas" | "field" | "-apple-system-background" => [255, 255, 255, 255],
        "canvastext" | "windowtext" | "fieldtext" => [0, 0, 0, 255],
        // the label color of macOS, 85% black
        "buttontext" | "-apple-system-label" => [0, 0, 0, 216],
        "linktext" | "-webkit-link" => [0, 0, 238, 255],
        "visitedtext" => [85, 26, 139, 255],
        "activetext" | "-webkit-activelink" => [255, 0, 0, 255],
        "graytext" => [109, 109, 109, 255],
        "buttonface" | "buttonborder" | "threedface" => [239, 239, 239, 255],
        "highlight" => [181, 213, 255, 255],
        "highlighttext" => [0, 0, 0, 255],
        "mark" => [255, 255, 0, 255],
        "marktext" => [0, 0, 0, 255],
        "-webkit-text" => [0, 0, 0, 255],
        _ => return None,
    })
}

fn rgba(c: &crate::color::Color) -> Option<SpecColor> {
    if c.space != Space::LegacyRgb {
        let mut s = String::new();
        c.write(Syntax::Legacy, &mut s);
        return Some(SpecColor::Other(s.into()));
    }
    let ch = c.channels();
    let v = |i: usize| ch[i].unwrap_or(0.0).round().clamp(0.0, 255.0) as u8;
    let a = (c.alpha.unwrap_or(0.0) * 255.0).round().clamp(0.0, 255.0) as u8;
    Some(SpecColor::Rgba([v(0), v(1), v(2), a]))
}

pub(crate) fn color(src: &str, x: &Item) -> Option<SpecColor> {
    if let Some(w) = x.ident(src) {
        if w == "currentcolor" {
            return Some(SpecColor::Current);
        }
        if let Some(c) = system_color(&w) {
            return Some(SpecColor::Rgba(c));
        }
    }
    let span = x.whole_in(src);
    match parse_color(src, span)? {
        ColorValue::Color(c) => rgba(&c),
        ColorValue::LightDark(b) => match (rgba(&b.0)?, rgba(&b.1)?) {
            (SpecColor::Rgba(l), SpecColor::Rgba(d)) => Some(SpecColor::LightDark(l, d)),
            (l, _) => Some(l),
        },
    }
}
