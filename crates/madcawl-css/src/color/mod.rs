//! `<color>` values of CSS Color 3, 4 and 5: named and hex colors,
//! `rgb()`, `hsl()`, `hwb()`, `lab()`, `lch()`, `oklab()`, `oklch()`,
//! `color()`, `device-cmyk()` and `light-dark()`, parsed from a span of
//! the source, and their serialization.
//!
//! The sRGB forms (names, hex, `rgb()`, `hsl()`, `hwb()`) become one
//! legacy sRGB color with channels from 0 to 255; the others keep their
//! own space and channels as given, `none` included.

mod named;
mod parse;

use std::fmt::Write;

pub use parse::{light_dark_arguments, parse_color};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    /// sRGB written the legacy way, channels 0 to 255
    LegacyRgb,
    Lab,
    Lch,
    Oklab,
    Oklch,
    Srgb,
    SrgbLinear,
    DisplayP3,
    A98Rgb,
    ProphotoRgb,
    Rec2020,
    XyzD50,
    XyzD65,
    DeviceCmyk,
    /// A `--name` profile from `@color-profile`
    Custom,
}

/// At most this many channels: a custom profile may have any number,
/// four is the most any defined space has.
pub const MAX_CHANNELS: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct Color {
    pub space: Space,
    /// The `--name` of a custom profile
    pub profile: Option<Box<str>>,
    /// `None` is the keyword `none`
    pub channels: [Option<f64>; MAX_CHANNELS],
    pub count: u8,
    /// From 0 to 1; `None` is `none`
    pub alpha: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ColorValue {
    Color(Color),
    /// `light-dark(light, dark)`
    LightDark(Box<(Color, Color)>),
}

/// How the sRGB colors are written out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    /// `rgb(r, g, b)` and `rgba(r, g, b, a)`, channels 0 to 255
    Legacy,
    /// `color(srgb r g b)`, channels 0 to 1
    ColorFunction,
}

/// A number as the CSS Color tests write it: rounded to 6 decimals
/// (half a unit of the 7th added first), with no `.0` on an integer and
/// an exponent below 1e-4, as Python's `str` of a float has.
pub fn write_number(v: f64, out: &mut String) {
    let rounded: f64 = format!("{:.6}", v + 1e-7).parse().unwrap_or(v);
    if rounded == 0.0 {
        out.push('0');
    } else if rounded.fract() == 0.0 && rounded.abs() < 1e16 {
        let _ = write!(out, "{}", rounded as i64);
    } else if rounded.abs() < 1e-4 {
        let e = format!("{rounded:e}");
        let (m, x) = e.split_once('e').unwrap_or((&e, "0"));
        let (sign, digits) = x.strip_prefix('-').map_or(("+", x), |d| ("-", d));
        let _ = write!(out, "{m}e{sign}{digits:0>2}");
    } else {
        let _ = write!(out, "{rounded}");
    }
}

fn write_channel(c: Option<f64>, out: &mut String) {
    match c {
        Some(v) => write_number(v, out),
        None => out.push_str("none"),
    }
}

impl Space {
    pub fn name(self) -> &'static str {
        match self {
            Space::LegacyRgb | Space::Srgb => "srgb",
            Space::Lab => "lab",
            Space::Lch => "lch",
            Space::Oklab => "oklab",
            Space::Oklch => "oklch",
            Space::SrgbLinear => "srgb-linear",
            Space::DisplayP3 => "display-p3",
            Space::A98Rgb => "a98-rgb",
            Space::ProphotoRgb => "prophoto-rgb",
            Space::Rec2020 => "rec2020",
            Space::XyzD50 => "xyz-d50",
            Space::XyzD65 => "xyz-d65",
            Space::DeviceCmyk => "device-cmyk",
            Space::Custom => "",
        }
    }
}

impl Color {
    pub fn channels(&self) -> &[Option<f64>] {
        &self.channels[..self.count as usize]
    }

    fn write_alpha(&self, sep: &str, out: &mut String) {
        if self.alpha != Some(1.0) {
            out.push_str(sep);
            write_channel(self.alpha, out);
        }
    }

    pub fn write(&self, syntax: Syntax, out: &mut String) {
        match self.space {
            Space::LegacyRgb if syntax == Syntax::Legacy => {
                out.push_str(if self.alpha == Some(1.0) {
                    "rgb("
                } else {
                    "rgba("
                });
                for (i, c) in self.channels().iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write_channel(*c, out);
                }
                self.write_alpha(", ", out);
            }
            Space::Lab | Space::Lch | Space::Oklab | Space::Oklch => {
                out.push_str(self.space.name());
                out.push('(');
                for (i, c) in self.channels().iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    write_channel(*c, out);
                }
                self.write_alpha(" / ", out);
            }
            _ => {
                out.push_str("color(");
                out.push_str(self.profile.as_deref().unwrap_or(self.space.name()));
                let scale = if self.space == Space::LegacyRgb {
                    255.0
                } else {
                    1.0
                };
                for c in self.channels() {
                    out.push(' ');
                    write_channel(c.map(|v| v / scale), out);
                }
                self.write_alpha(" / ", out);
            }
        }
        out.push(')');
    }
}
