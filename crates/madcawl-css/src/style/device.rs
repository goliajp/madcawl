//! The device a page is styled for.

/// What media queries are evaluated against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Device {
    /// The viewport, in CSS pixels
    pub width: f64,
    pub height: f64,
    /// Device pixels per CSS pixel
    pub scale: f64,
    pub dark: bool,
    /// `print` media instead of `screen`
    pub print: bool,
    pub scripting: bool,
}

impl Default for Device {
    fn default() -> Device {
        Device {
            width: 1280.0,
            height: 800.0,
            scale: 1.0,
            dark: false,
            print: false,
            scripting: false,
        }
    }
}

/// A length in CSS pixels, for media queries and computed values: font
/// relative units against the initial font size.
pub(crate) fn length_px(n: f64, unit: &str, d: &Device) -> Option<f64> {
    Some(match unit {
        "px" => n,
        "em" | "rem" => n * 16.0,
        "ex" | "rex" => n * 8.0,
        "ch" | "rch" => n * 8.0,
        "vw" | "svw" | "lvw" | "dvw" => n * d.width / 100.0,
        "vh" | "svh" | "lvh" | "dvh" => n * d.height / 100.0,
        "vmin" => n * d.width.min(d.height) / 100.0,
        "vmax" => n * d.width.max(d.height) / 100.0,
        "cm" => n * 96.0 / 2.54,
        "mm" => n * 96.0 / 25.4,
        "q" => n * 96.0 / 101.6,
        "in" => n * 96.0,
        "pt" => n * 96.0 / 72.0,
        "pc" => n * 16.0,
        _ => return None,
    })
}
