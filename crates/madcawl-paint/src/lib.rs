//! Painting: the fragment tree to a display list in CSS paint order (`build`), and an offscreen raster of a
//! display list into a BGRA sRGB bitmap written as PNG (`raster`). The display list (`list`) is the boundary
//! between the two: it refers to faces and glyph ids, never to the DOM, the styles or the fragment tree.
//!
//! The raster here draws on the CPU through CoreGraphics and CoreText. It reads nothing but the display list
//! and the faces its glyph runs name, so a compositor drawing the same list on the GPU can take its place.

mod build;
pub mod list;
pub mod raster;

pub use build::paint;
pub use list::{DisplayList, Entry, GlyphPos, Item, Line, RectF, Rgba};
pub use raster::{Bitmap, Raster};
