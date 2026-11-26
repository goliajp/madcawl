//! Reference marks drawn over a bitmap the raster already drew: an outline around each element and a label
//! with its reference. They are drawn from the list of marks the caller gives, after the page, so page
//! content can neither draw nor hide one.

use super::ffi::{self, Ref};
use super::{Bitmap, Raster, context, fill};
use crate::list::{RectF, Rgba};

/// An element to mark: its rectangle in CSS px on the page and the label to draw on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub rect: RectF,
    pub label: String,
}

/// The most marks drawn on one bitmap; the rest are left out.
pub const MAX_MARKS: usize = 2048;

// label text size, label padding and outline width, in device px at scale 1
const LABEL_PX: f32 = 11.0;
const PAD_PX: f32 = 2.0;
const STROKE_PX: f32 = 2.0;
// strong colors that white text reads on, taken in turn
const COLORS: [Rgba; 6] = [
    Rgba([214, 39, 40, 255]),
    Rgba([31, 119, 180, 255]),
    Rgba([44, 140, 44, 255]),
    Rgba([148, 73, 189, 255]),
    Rgba([200, 100, 0, 255]),
    Rgba([0, 128, 128, 255]),
];

/// A label's text as glyphs of the label face, with their advances.
struct Label {
    glyphs: Vec<u16>,
    advances: Vec<f64>,
    width: f32,
}

fn shape(font: Ref, text: &str) -> Label {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut glyphs = vec![0u16; units.len()];
    let mut sizes = vec![ffi::CGSize::default(); units.len()];
    // SAFETY: both buffers hold one entry per UTF-16 unit; a unit the face lacks gets glyph 0
    let width = unsafe {
        ffi::CTFontGetGlyphsForCharacters(
            font,
            units.as_ptr(),
            glyphs.as_mut_ptr(),
            units.len() as isize,
        );
        ffi::CTFontGetAdvancesForGlyphs(
            font,
            0,
            glyphs.as_ptr(),
            sizes.as_mut_ptr(),
            glyphs.len() as isize,
        )
    };
    Label {
        glyphs,
        advances: sizes.iter().map(|s| s.width).collect(),
        width: width as f32,
    }
}

/// Where a label of `w` by `h` goes for `r`: above its top left corner, inside it, or below it, whichever
/// first stays in `view` and clear of the labels already placed; else above.
fn place(r: RectF, w: f32, h: f32, view: RectF, taken: &[RectF]) -> RectF {
    let x = r.x.clamp(view.x, (view.right() - w).max(view.x));
    let spots = [r.y - h, r.y, r.bottom()];
    let inside = |c: &RectF| c.y >= view.y && c.bottom() <= view.bottom();
    let at = |y: f32| RectF { x, y, w, h };
    spots
        .iter()
        .map(|&y| at(y))
        .find(|c| inside(c) && !taken.iter().any(|t| t.intersects(c)))
        .unwrap_or_else(|| at(spots[0].clamp(view.y, (view.bottom() - h).max(view.y))))
}

/// `px` device pixels in CSS px at `scale`: the same on every bitmap below scale 1, growing with the scale
/// to half again above it, so a mark stays readable on a shrunken picture.
fn device(px: f32, scale: f32) -> f32 {
    let s = scale.max(0.01);
    (px * s).clamp(px, px * 1.5) / s
}

impl Raster {
    /// Draws `marks` over `bm`, which this raster drew; marks past [`MAX_MARKS`] are left out. The count drawn.
    pub fn draw_marks(&self, bm: &mut Bitmap, marks: &[Mark]) -> usize {
        let marks = &marks[..marks.len().min(MAX_MARKS)];
        let name = b"Menlo-Bold";
        let mut taken: Vec<RectF> = Vec::with_capacity(marks.len());
        // SAFETY: the context draws into `bm`, which outlives it; every object made is released
        unsafe {
            let ctx = context(bm, self.scale, self.view);
            let cf = ffi::CFStringCreateWithBytes(
                std::ptr::null(),
                name.as_ptr(),
                name.len() as isize,
                ffi::kCFStringEncodingUTF8,
                0,
            );
            let (size, pad) = (device(LABEL_PX, self.scale), device(PAD_PX, self.scale));
            let stroke = device(STROKE_PX, self.scale);
            let font = ffi::CTFontCreateWithName(cf, size as f64, std::ptr::null());
            ffi::CFRelease(cf);
            let ascent = ffi::CTFontGetAscent(font) as f32;
            let h = ascent + ffi::CTFontGetDescent(font) as f32 + 2.0 * pad;
            for (i, m) in marks.iter().enumerate() {
                let color = COLORS[i % COLORS.len()];
                outline(ctx, m.rect, color, stroke);
                let label = shape(font, &m.label);
                let at = place(m.rect, label.width + 2.0 * pad, h, self.view, &taken);
                taken.push(at);
                fill(ctx, color);
                ffi::CGContextFillRect(ctx, ffi::rect(at.x, at.y, at.w, at.h));
                text(ctx, font, &label, at.x + pad, at.y + pad + ascent);
            }
            ffi::CFRelease(font);
            ffi::CGContextRelease(ctx);
        }
        marks.len()
    }
}

// SAFETY: `ctx` must be a live bitmap context
unsafe fn outline(ctx: Ref, r: RectF, c: Rgba, width: f32) {
    // SAFETY: as the caller promises
    unsafe {
        let (red, g, b, _) = super::rgb(c);
        ffi::CGContextSetRGBStrokeColor(ctx, red, g, b, 1.0);
        // inset by half the width, so the stroke lies inside the element's box
        let r = ffi::rect(
            r.x + width / 2.0,
            r.y + width / 2.0,
            (r.w - width).max(0.0),
            (r.h - width).max(0.0),
        );
        ffi::CGContextStrokeRectWithWidth(ctx, r, width as f64);
    }
}

// SAFETY: `ctx` must be a live bitmap context and `font` a live CTFont
unsafe fn text(ctx: Ref, font: Ref, l: &Label, x: f32, baseline: f32) {
    let mut pen = x as f64;
    let points: Vec<ffi::CGPoint> = l
        .advances
        .iter()
        .map(|a| {
            let p = ffi::CGPoint {
                x: pen,
                y: -baseline as f64,
            };
            pen += a;
            p
        })
        .collect();
    // SAFETY: as the caller promises; `points` holds one entry per glyph
    unsafe {
        fill(ctx, Rgba::WHITE);
        ffi::CGContextSaveGState(ctx);
        // glyphs are drawn with y up, so they stand upright
        ffi::CGContextScaleCTM(ctx, 1.0, -1.0);
        ffi::CTFontDrawGlyphs(
            font,
            l.glyphs.as_ptr(),
            points.as_ptr(),
            l.glyphs.len(),
            ctx,
        );
        ffi::CGContextRestoreGState(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: RectF = RectF {
        x: 0.0,
        y: 100.0,
        w: 200.0,
        h: 100.0,
    };

    #[test]
    fn labels_go_above_then_inside_then_below_and_stay_in_view() {
        let r = RectF {
            x: 10.0,
            y: 150.0,
            w: 50.0,
            h: 20.0,
        };
        let a = place(r, 20.0, 15.0, VIEW, &[]);
        assert_eq!((a.x, a.y), (10.0, 135.0));
        let b = place(r, 20.0, 15.0, VIEW, &[a]);
        assert_eq!(b.y, 150.0);
        let c = place(r, 20.0, 15.0, VIEW, &[a, b]);
        assert_eq!(c.y, 170.0);
        // at the top of the view there is no room above
        let top = RectF { y: 100.0, ..r };
        assert_eq!(place(top, 20.0, 15.0, VIEW, &[]).y, 100.0);
        // at the right edge the label moves left to stay in
        let right = RectF { x: 195.0, ..r };
        assert_eq!(place(right, 20.0, 15.0, VIEW, &[]).x, 180.0);
    }
}
