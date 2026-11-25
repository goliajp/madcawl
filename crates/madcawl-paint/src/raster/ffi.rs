//! The CoreGraphics, CoreText and ImageIO functions the raster calls, declared by hand.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::c_void;

pub type Ref = *const c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGAffineTransform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

pub const kCGImageAlphaPremultipliedFirst: u32 = 2;
pub const kCGBitmapByteOrder32Little: u32 = 2 << 12;
pub const kCGLineCapRound: i32 = 1;
pub const kCGLineCapButt: i32 = 0;
pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFRelease(cf: Ref);
    pub fn CFStringCreateWithBytes(
        alloc: Ref,
        bytes: *const u8,
        len: isize,
        enc: u32,
        ext: u8,
    ) -> Ref;
    pub fn CFURLCreateFromFileSystemRepresentation(
        alloc: Ref,
        buf: *const u8,
        len: isize,
        dir: u8,
    ) -> Ref;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub static kCGColorSpaceSRGB: Ref;
    pub fn CGColorSpaceCreateWithName(name: Ref) -> Ref;
    pub fn CGColorSpaceRelease(cs: Ref);
    pub fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits: usize,
        row: usize,
        space: Ref,
        info: u32,
    ) -> Ref;
    pub fn CGDataProviderCreateWithData(
        info: *mut c_void,
        data: *const c_void,
        size: usize,
        release: Ref,
    ) -> Ref;
    pub fn CGDataProviderRelease(p: Ref);
    pub fn CGImageCreate(
        width: usize,
        height: usize,
        bits: usize,
        bits_per_pixel: usize,
        row: usize,
        space: Ref,
        info: u32,
        provider: Ref,
        decode: *const f64,
        interpolate: bool,
        intent: i32,
    ) -> Ref;
    pub fn CGContextRelease(ctx: Ref);
    pub fn CGImageRelease(img: Ref);
    pub fn CGContextSaveGState(ctx: Ref);
    pub fn CGContextRestoreGState(ctx: Ref);
    pub fn CGContextClipToRect(ctx: Ref, r: CGRect);
    pub fn CGContextTranslateCTM(ctx: Ref, x: f64, y: f64);
    pub fn CGContextScaleCTM(ctx: Ref, x: f64, y: f64);
    pub fn CGContextSetTextMatrix(ctx: Ref, t: CGAffineTransform);
    pub fn CGContextSetShouldAntialias(ctx: Ref, on: bool);
    pub fn CGContextSetAllowsFontSmoothing(ctx: Ref, on: bool);
    pub fn CGContextSetShouldSmoothFonts(ctx: Ref, on: bool);
    pub fn CGContextSetAllowsFontSubpixelPositioning(ctx: Ref, on: bool);
    pub fn CGContextSetShouldSubpixelPositionFonts(ctx: Ref, on: bool);
    pub fn CGContextSetShouldSubpixelQuantizeFonts(ctx: Ref, on: bool);
    pub fn CGContextSetRGBFillColor(ctx: Ref, r: f64, g: f64, b: f64, a: f64);
    pub fn CGContextSetRGBStrokeColor(ctx: Ref, r: f64, g: f64, b: f64, a: f64);
    pub fn CGContextFillRect(ctx: Ref, r: CGRect);
    pub fn CGContextStrokeRectWithWidth(ctx: Ref, r: CGRect, w: f64);
    pub fn CGContextBeginPath(ctx: Ref);
    pub fn CGContextMoveToPoint(ctx: Ref, x: f64, y: f64);
    pub fn CGContextAddLineToPoint(ctx: Ref, x: f64, y: f64);
    pub fn CGContextClosePath(ctx: Ref);
    pub fn CGContextFillPath(ctx: Ref);
    pub fn CGContextStrokePath(ctx: Ref);
    pub fn CGContextSetLineWidth(ctx: Ref, w: f64);
    pub fn CGContextSetLineCap(ctx: Ref, cap: i32);
    pub fn CGContextSetLineDash(ctx: Ref, phase: f64, lengths: *const f64, n: usize);
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    pub fn CTFontDrawGlyphs(
        font: Ref,
        glyphs: *const u16,
        positions: *const CGPoint,
        n: usize,
        ctx: Ref,
    );
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    pub fn CGImageDestinationCreateWithURL(url: Ref, kind: Ref, n: usize, opts: Ref) -> Ref;
    pub fn CGImageDestinationAddImage(dest: Ref, img: Ref, props: Ref);
    pub fn CGImageDestinationFinalize(dest: Ref) -> bool;
}

pub fn rect(x: f32, y: f32, w: f32, h: f32) -> CGRect {
    CGRect {
        origin: CGPoint {
            x: x as f64,
            y: y as f64,
        },
        size: CGSize {
            width: w as f64,
            height: h as f64,
        },
    }
}
