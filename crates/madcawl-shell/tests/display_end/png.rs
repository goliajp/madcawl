//! A PNG read back to its pixels with ImageIO, as decoded and with no
//! color matching, so a PNG the raster wrote compares byte for byte.

#![allow(non_snake_case)]

use std::ffi::c_void;

type Ref = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataCreate(alloc: Ref, bytes: *const u8, len: isize) -> Ref;
    fn CFDataGetLength(d: Ref) -> isize;
    fn CFDataGetBytePtr(d: Ref) -> *const u8;
    fn CFRelease(r: Ref);
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGImageGetWidth(i: Ref) -> usize;
    fn CGImageGetHeight(i: Ref) -> usize;
    fn CGImageGetBitsPerPixel(i: Ref) -> usize;
    fn CGImageGetBitsPerComponent(i: Ref) -> usize;
    fn CGImageGetBytesPerRow(i: Ref) -> usize;
    fn CGImageGetAlphaInfo(i: Ref) -> u32;
    fn CGImageGetDataProvider(i: Ref) -> Ref;
    fn CGDataProviderCopyData(p: Ref) -> Ref;
    fn CGImageRelease(i: Ref);
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithData(d: Ref, options: Ref) -> Ref;
    fn CGImageSourceCreateImageAtIndex(s: Ref, i: usize, options: Ref) -> Ref;
}

/// A decoded picture: width, height and (r, g, b, a) a pixel, top row
/// first.
pub struct Rgba {
    pub width: usize,
    pub height: usize,
    pub px: Vec<[u8; 4]>,
}

/// The pixels of `png`, 8 bits a channel, RGB or RGBA as ImageIO
/// decodes a PNG the raster wrote.
pub fn decode(png: &[u8]) -> Rgba {
    // SAFETY: each object is checked to exist, read while held and released once
    unsafe {
        let data = CFDataCreate(std::ptr::null(), png.as_ptr(), png.len() as isize);
        let src = CGImageSourceCreateWithData(data, std::ptr::null());
        assert!(!src.is_null(), "not a picture ImageIO reads");
        let img = CGImageSourceCreateImageAtIndex(src, 0, std::ptr::null());
        assert!(!img.is_null());
        let (w, h) = (CGImageGetWidth(img), CGImageGetHeight(img));
        let (bpp, bpc, row) = (CGImageGetBitsPerPixel(img), CGImageGetBitsPerComponent(img), CGImageGetBytesPerRow(img));
        assert_eq!(bpc, 8, "8 bits a channel");
        // 3 alpha last, 1 premultiplied last, 5 skip last, 0 none
        let alpha = CGImageGetAlphaInfo(img);
        assert!(matches!((bpp, alpha), (32, 1 | 3 | 5) | (24, 0)), "a layout this reader takes: {bpp} bpp, alpha {alpha}");
        let pixels = CGDataProviderCopyData(CGImageGetDataProvider(img));
        let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(pixels), CFDataGetLength(pixels) as usize);
        let step = bpp / 8;
        let mut px = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let p = &bytes[y * row + x * step..];
                let a = if bpp == 32 && alpha != 5 { p[3] } else { 255 };
                px.push([p[0], p[1], p[2], a]);
            }
        }
        CFRelease(pixels);
        CGImageRelease(img);
        CFRelease(src);
        CFRelease(data);
        Rgba { width: w, height: h, px }
    }
}
