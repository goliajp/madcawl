//! Text for layout: font faces, metrics and fallback through CoreText, a bounded shaping cache, and inline text
//! itemized by bidi level, script and style, shaped by word and broken into lines.
//!
//! Shaping uses only CoreText's public `CTLine` / `CTRun` interface. Glyph rasterization is not here.

mod cache;
mod ffi;
mod font;
mod lru;
mod para;
mod shape;
mod shaper;

pub use cache::{CacheStats, ShapeCache, Shaped};
pub use font::{FaceId, Fonts, Metrics};
pub use madcawl_unicode::Direction;
pub use para::{After, Kind, Line, Paragraph, Segment, Span};
pub use shape::{Glyph, NO_LIGATURES};
pub use shaper::{CACHE_ENTRIES, CACHE_WEIGHT, Shaper, Style};

#[cfg(test)]
mod tests;
