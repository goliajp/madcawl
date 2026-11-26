//! Fonts a suite expects installed, registered for this process only.

use std::path::Path;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFURLCreateFromFileSystemRepresentation(
        alloc: *const u8,
        buf: *const u8,
        len: isize,
        dir: u8,
    ) -> *const u8;
    fn CFRelease(cf: *const u8);
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    fn CTFontManagerRegisterFontsForURL(url: *const u8, scope: u32, error: *mut *const u8) -> bool;
}

/// Makes the fonts in the file at `path` available to this process only.
pub fn register_font(path: &Path) -> bool {
    const PROCESS: u32 = 1;
    let b = path.as_os_str().as_encoded_bytes();
    // SAFETY: the path bytes are valid for the length given; the URL is released after the call
    unsafe {
        let url = CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            b.as_ptr(),
            b.len() as isize,
            0,
        );
        if url.is_null() {
            return false;
        }
        let ok = CTFontManagerRegisterFontsForURL(url, PROCESS, std::ptr::null_mut());
        CFRelease(url);
        ok
    }
}
