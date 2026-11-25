//! A bitmap to a PNG file through ImageIO.

use std::io;
use std::path::Path;

use super::Bitmap;
use super::ffi::{self, Ref};

// releases a CF object when it goes out of scope
struct Owned(Ref);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer came from a Create call and is released once
            unsafe { ffi::CFRelease(self.0) }
        }
    }
}

fn fail(what: &str) -> io::Error {
    io::Error::other(format!("ImageIO: {what}"))
}

/// Writes `bm` to `path` as an sRGB PNG with alpha.
pub fn write_png(bm: &Bitmap, path: &Path) -> io::Result<()> {
    let bytes = path.as_os_str().as_encoded_bytes();
    let kind = b"public.png";
    // SAFETY: every buffer is valid for the length given and outlives what reads it: the image reads the
    // bitmap's pixels in place, and it is released before this returns; each object made is released
    unsafe {
        let url = Owned(ffi::CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            0,
        ));
        let ty = Owned(ffi::CFStringCreateWithBytes(
            std::ptr::null(),
            kind.as_ptr(),
            kind.len() as isize,
            ffi::kCFStringEncodingUTF8,
            0,
        ));
        if url.0.is_null() || ty.0.is_null() {
            return Err(fail("the file name"));
        }
        let provider = ffi::CGDataProviderCreateWithData(
            std::ptr::null_mut(),
            bm.data.as_ptr().cast(),
            bm.data.len(),
            std::ptr::null(),
        );
        if provider.is_null() {
            return Err(fail("a data provider of the bitmap"));
        }
        let cs = ffi::CGColorSpaceCreateWithName(ffi::kCGColorSpaceSRGB);
        let img = ffi::CGImageCreate(
            bm.width,
            bm.height,
            8,
            32,
            bm.width * 4,
            cs,
            ffi::kCGImageAlphaPremultipliedFirst | ffi::kCGBitmapByteOrder32Little,
            provider,
            std::ptr::null(),
            false,
            0,
        );
        ffi::CGColorSpaceRelease(cs);
        ffi::CGDataProviderRelease(provider);
        if img.is_null() {
            return Err(fail("an image of the bitmap"));
        }
        let dest = Owned(ffi::CGImageDestinationCreateWithURL(
            url.0,
            ty.0,
            1,
            std::ptr::null(),
        ));
        if dest.0.is_null() {
            ffi::CGImageRelease(img);
            return Err(fail("a destination at the path"));
        }
        ffi::CGImageDestinationAddImage(dest.0, img, std::ptr::null());
        let ok = ffi::CGImageDestinationFinalize(dest.0);
        ffi::CGImageRelease(img);
        if !ok {
            return Err(fail("writing the file"));
        }
    }
    Ok(())
}
