//! Borders: each side a trapezoid from the border box edge to the padding box edge, mitred at the corners,
//! drawn as bands across its width for the line styles made of several.

use super::ffi::{self, Ref};
use super::fill;
use crate::list::{Line, RectF, Rgba};

type P = (f32, f32);

fn lerp(a: P, b: P, t: f32) -> P {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

// the outer and inner corners at the start and end of side `k` (top, right, bottom, left, clockwise)
fn side(r: RectF, w: [f32; 4], k: usize) -> [P; 4] {
    let (x, y, rt, b) = (r.x, r.y, r.right(), r.bottom());
    let (ix, iy, irt, ib) = (x + w[3], y + w[0], rt - w[1], b - w[2]);
    match k {
        0 => [(x, y), (rt, y), (irt, iy), (ix, iy)],
        1 => [(rt, y), (rt, b), (irt, ib), (irt, iy)],
        2 => [(rt, b), (x, b), (ix, ib), (irt, ib)],
        _ => [(x, b), (x, y), (ix, iy), (ix, ib)],
    }
}

fn shade(c: Rgba, k: f32) -> Rgba {
    let [r, g, b, a] = c.0;
    let f = |v: u8| (v as f32 * k).round().clamp(0.0, 255.0) as u8;
    Rgba([f(r), f(g), f(b), a])
}

// SAFETY: `ctx` must be a live context
unsafe fn band(ctx: Ref, q: [P; 4], t0: f32, t1: f32, c: Rgba) {
    let [o1, o2, i2, i1] = q;
    let pts = [
        lerp(o1, i1, t0),
        lerp(o2, i2, t0),
        lerp(o2, i2, t1),
        lerp(o1, i1, t1),
    ];
    fill(ctx, c);
    // SAFETY: as the caller promises
    unsafe {
        ffi::CGContextBeginPath(ctx);
        ffi::CGContextMoveToPoint(ctx, pts[0].0 as f64, pts[0].1 as f64);
        for p in &pts[1..] {
            ffi::CGContextAddLineToPoint(ctx, p.0 as f64, p.1 as f64);
        }
        ffi::CGContextClosePath(ctx);
        ffi::CGContextFillPath(ctx);
    }
}

// SAFETY: `ctx` must be a live context
unsafe fn dashes(ctx: Ref, q: [P; 4], w: f32, c: Rgba, dotted: bool) {
    let [o1, o2, i2, i1] = q;
    let (a, b) = (lerp(o1, i1, 0.5), lerp(o2, i2, 0.5));
    let (r, g, bl, al) = super::rgb(c);
    let (cap, lengths) = if dotted {
        (ffi::kCGLineCapRound, [0.0, 2.0 * w as f64])
    } else {
        (ffi::kCGLineCapButt, [3.0 * w as f64, 3.0 * w as f64])
    };
    // SAFETY: as the caller promises; the dash array has two entries
    unsafe {
        ffi::CGContextSaveGState(ctx);
        ffi::CGContextSetRGBStrokeColor(ctx, r, g, bl, al);
        ffi::CGContextSetLineWidth(ctx, w as f64);
        ffi::CGContextSetLineCap(ctx, cap);
        ffi::CGContextSetLineDash(ctx, 0.0, lengths.as_ptr(), 2);
        ffi::CGContextBeginPath(ctx);
        ffi::CGContextMoveToPoint(ctx, a.0 as f64, a.1 as f64);
        ffi::CGContextAddLineToPoint(ctx, b.0 as f64, b.1 as f64);
        ffi::CGContextStrokePath(ctx);
        ffi::CGContextRestoreGState(ctx);
    }
}

/// Draws the border of box `r`.
///
/// # Safety
/// `ctx` must be a live context.
pub unsafe fn draw(ctx: Ref, r: RectF, w: [f32; 4], c: [Rgba; 4], lines: [Line; 4]) {
    for k in 0..4 {
        if w[k] <= 0.0 {
            continue;
        }
        let q = side(r, w, k);
        // the top and left sides are the ones in shadow for `inset` and `groove`
        let lit = k == 1 || k == 2;
        let (dark, light) = (shade(c[k], 0.6), c[k]);
        // SAFETY: as the caller promises
        unsafe {
            match lines[k] {
                Line::Solid => band(ctx, q, 0.0, 1.0, c[k]),
                Line::Double if w[k] >= 3.0 => {
                    band(ctx, q, 0.0, 1.0 / 3.0, c[k]);
                    band(ctx, q, 2.0 / 3.0, 1.0, c[k]);
                }
                Line::Double => band(ctx, q, 0.0, 1.0, c[k]),
                Line::Dashed => dashes(ctx, q, w[k], c[k], false),
                Line::Dotted => dashes(ctx, q, w[k], c[k], true),
                Line::Inset => band(ctx, q, 0.0, 1.0, if lit { light } else { dark }),
                Line::Outset => band(ctx, q, 0.0, 1.0, if lit { dark } else { light }),
                Line::Groove | Line::Ridge => {
                    let outer_dark = (lines[k] == Line::Groove) != lit;
                    let (a, b) = if outer_dark {
                        (dark, light)
                    } else {
                        (light, dark)
                    };
                    band(ctx, q, 0.0, 0.5, a);
                    band(ctx, q, 0.5, 1.0, b);
                }
            }
        }
    }
}
