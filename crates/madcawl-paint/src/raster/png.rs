//! A bitmap to PNG through ImageIO: into a file, or into memory.

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

fn png_type() -> Owned {
    let kind = b"public.png";
    // SAFETY: the bytes are valid for the length given
    Owned(unsafe {
        ffi::CFStringCreateWithBytes(
            std::ptr::null(),
            kind.as_ptr(),
            kind.len() as isize,
            ffi::kCFStringEncodingUTF8,
            0,
        )
    })
}

/// Adds `bm` to the destination `make` creates and finalizes it.
fn encode_into(bm: &Bitmap, make: impl FnOnce(Ref) -> Ref) -> io::Result<()> {
    let ty = png_type();
    if ty.0.is_null() {
        return Err(fail("the image type"));
    }
    // SAFETY: the image reads the bitmap's pixels in place, and it is released before this returns; each
    // object made is released
    unsafe {
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
        let dest = Owned(make(ty.0));
        if dest.0.is_null() {
            ffi::CGImageRelease(img);
            return Err(fail("a destination"));
        }
        ffi::CGImageDestinationAddImage(dest.0, img, std::ptr::null());
        let ok = ffi::CGImageDestinationFinalize(dest.0);
        ffi::CGImageRelease(img);
        if !ok {
            return Err(fail("writing the image"));
        }
    }
    Ok(())
}

/// Writes `bm` to `path` as an sRGB PNG with alpha.
pub fn write_png(bm: &Bitmap, path: &Path) -> io::Result<()> {
    let bytes = path.as_os_str().as_encoded_bytes();
    // SAFETY: the path bytes are valid for the length given
    let url = Owned(unsafe {
        ffi::CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            0,
        )
    });
    if url.0.is_null() {
        return Err(fail("the file name"));
    }
    // SAFETY: `url` and the type live through the call
    encode_into(bm, |ty| unsafe {
        ffi::CGImageDestinationCreateWithURL(url.0, ty, 1, std::ptr::null())
    })
}

/// `bm` as the bytes of an sRGB PNG with alpha.
pub fn encode_png(bm: &Bitmap) -> io::Result<Vec<u8>> {
    // SAFETY: a growable data object, released when `data` drops
    let data = Owned(unsafe { ffi::CFDataCreateMutable(std::ptr::null(), 0) });
    if data.0.is_null() {
        return Err(fail("a data object"));
    }
    // SAFETY: `data` and the type live through the call
    encode_into(bm, |ty| unsafe {
        ffi::CGImageDestinationCreateWithData(data.0, ty, 1, std::ptr::null())
    })?;
    // SAFETY: the data object holds `len` bytes at the pointer while it lives
    let out = unsafe {
        let len = ffi::CFDataGetLength(data.0) as usize;
        std::slice::from_raw_parts(ffi::CFDataGetBytePtr(data.0), len).to_vec()
    };
    Ok(out)
}
