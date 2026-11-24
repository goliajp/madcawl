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
