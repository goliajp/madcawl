//! An offscreen raster of a display list: CoreGraphics fills and strokes, CoreText draws glyph runs, into a
//! premultiplied BGRA bitmap tagged sRGB; ImageIO writes it as PNG.

mod border;
mod ffi;
mod png;

use madcawl_text::Fonts;

use crate::list::{DisplayList, Item, RectF, Rgba};
use ffi::Ref;

pub use png::write_png;

/// The largest bitmap side drawn, in device pixels; a taller area is cut there.
pub const MAX_SIDE: usize = 16384;
/// The most device pixels a bitmap holds (256 MiB of BGRA); a taller area is cut where it reaches them.
pub const MAX_PIXELS: usize = 64 << 20;

/// Pixels, premultiplied BGRA, top row first.
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl Bitmap {
    /// The pixel at device `(x, y)` as (b, g, r, a).
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.width + x) * 4;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }
}

/// What part of the page a raster draws, and at how many device pixels per CSS px.
#[derive(Clone, Copy, Debug)]
pub struct Raster {
    pub view: RectF,
    pub scale: f32,
}

pub(crate) fn rgb(c: Rgba) -> (f64, f64, f64, f64) {
    let [r, g, b, a] = c.0;
    let f = |v: u8| v as f64 / 255.0;
    (f(r), f(g), f(b), f(a))
}

pub(crate) fn fill(ctx: Ref, c: Rgba) {
    let (r, g, b, a) = rgb(c);
    // SAFETY: `ctx` is a live bitmap context
    unsafe { ffi::CGContextSetRGBFillColor(ctx, r, g, b, a) }
}

// buffers glyph runs are handed to CoreText in, reused across runs
#[derive(Default)]
struct Scratch {
    ids: Vec<u16>,
    points: Vec<ffi::CGPoint>,
}

impl Raster {
    /// The bitmap's width and height in device pixels, under [`MAX_SIDE`] and [`MAX_PIXELS`].
    pub fn device_size(&self) -> (usize, usize) {
        let s = self.scale.max(0.01);
        let width = ((self.view.w * s).round() as usize).clamp(1, MAX_SIDE);
        let height =
            ((self.view.h * s).round() as usize).clamp(1, MAX_SIDE.min(MAX_PIXELS / width));
        (width, height)
    }

    /// Draws `list` with the faces of `fonts`, the ones its glyph runs were shaped with.
    pub fn draw(&self, list: &DisplayList, fonts: &mut Fonts) -> Bitmap {
        let s = self.scale.max(0.01);
        let (width, height) = self.device_size();
        let mut data = vec![0u8; width * height * 4];
        // SAFETY: `data` outlives the context, which is released below; every CF object made is released
        unsafe {
            let cs = ffi::CGColorSpaceCreateWithName(ffi::kCGColorSpaceSRGB);
            let ctx = ffi::CGBitmapContextCreate(
                data.as_mut_ptr().cast(),
                width,
                height,
                8,
                width * 4,
                cs,
                ffi::kCGImageAlphaPremultipliedFirst | ffi::kCGBitmapByteOrder32Little,
            );
            ffi::CGColorSpaceRelease(cs);
            assert!(!ctx.is_null(), "a bitmap context of {width}x{height}");
            setup(ctx, height, s, self.view);
            fill(ctx, list.canvas);
            ffi::CGContextFillRect(
                ctx,
                ffi::rect(self.view.x, self.view.y, self.view.w, self.view.h),
            );
            let mut scratch = Scratch::default();
            let mut clip = 0u32;
            for e in &list.entries {
                if !e.bounds.intersects(&self.view) {
                    continue;
                }
                if e.clip != clip {
                    if clip != 0 {
                        ffi::CGContextRestoreGState(ctx);
                    }
                    clip = e.clip;
                    if clip != 0 {
                        let r = list.clips[clip as usize].rect;
                        ffi::CGContextSaveGState(ctx);
                        ffi::CGContextClipToRect(ctx, ffi::rect(r.x, r.y, r.w, r.h));
                    }
                }
                item(ctx, list, &e.item, fonts, &mut scratch);
            }
            if clip != 0 {
                ffi::CGContextRestoreGState(ctx);
            }
            ffi::CGContextRelease(ctx);
        }
        Bitmap {
            width,
            height,
            data,
        }
    }
}

// SAFETY: `ctx` must be a live bitmap context `height` device pixels tall
unsafe fn setup(ctx: Ref, height: usize, s: f32, view: RectF) {
    // SAFETY: as the caller promises
    unsafe {
        // CSS px with y down: CoreGraphics' origin is the bottom left
        ffi::CGContextTranslateCTM(ctx, 0.0, height as f64);
        ffi::CGContextScaleCTM(ctx, s as f64, -s as f64);
        ffi::CGContextTranslateCTM(ctx, -view.x as f64, -view.y as f64);
        let identity = ffi::CGAffineTransform {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        };
        ffi::CGContextSetTextMatrix(ctx, identity);
        ffi::CGContextSetShouldAntialias(ctx, true);
        ffi::CGContextSetAllowsFontSmoothing(ctx, false);
        ffi::CGContextSetShouldSmoothFonts(ctx, false);
        ffi::CGContextSetAllowsFontSubpixelPositioning(ctx, true);
        ffi::CGContextSetShouldSubpixelPositionFonts(ctx, true);
        ffi::CGContextSetShouldSubpixelQuantizeFonts(ctx, true);
    }
}

// SAFETY: `ctx` must be a live bitmap context
unsafe fn item(ctx: Ref, list: &DisplayList, it: &Item, fonts: &mut Fonts, sc: &mut Scratch) {
    // SAFETY: as the caller promises; the glyph buffers hold `n` entries each
    unsafe {
        match it {
            Item::Rect { rect, color } | Item::Decoration { rect, color } => {
                fill(ctx, *color);
                ffi::CGContextFillRect(ctx, ffi::rect(rect.x, rect.y, rect.w, rect.h));
            }
            Item::Border {
                rect,
                widths,
                colors,
                lines,
            } => border::draw(ctx, *rect, *widths, *colors, *lines),
            Item::Image { rect } => {
                ffi::CGContextSetRGBStrokeColor(ctx, 0.78, 0.78, 0.78, 1.0);
                let r = ffi::rect(rect.x + 0.5, rect.y + 0.5, rect.w - 1.0, rect.h - 1.0);
                ffi::CGContextStrokeRectWithWidth(ctx, r, 1.0);
            }
            Item::Glyphs {
                face,
                size,
                color,
                glyphs,
            } => {
                let g = &list.glyphs[glyphs.start as usize..glyphs.end as usize];
                sc.ids.clear();
                sc.points.clear();
                sc.ids.extend(g.iter().map(|g| g.id));
                // glyphs are drawn with y up, so they stand upright
                sc.points.extend(g.iter().map(|g| ffi::CGPoint {
                    x: g.x as f64,
                    y: -g.y as f64,
                }));
                fill(ctx, *color);
                let n = g.len();
                ffi::CGContextSaveGState(ctx);
                ffi::CGContextScaleCTM(ctx, 1.0, -1.0);
                fonts.with_ct_font(*face, *size, |f| {
                    ffi::CTFontDrawGlyphs(f, sc.ids.as_ptr(), sc.points.as_ptr(), n, ctx)
                });
                ffi::CGContextRestoreGState(ctx);
            }
        }
    }
}
