//! Random bytes from the operating system, declared here: the crate
//! depends on no binding crate.

use std::ffi::c_void;

#[cfg(target_vendor = "apple")]
unsafe extern "C" {
    fn arc4random_buf(buf: *mut c_void, len: usize);
}

#[cfg(not(target_vendor = "apple"))]
unsafe extern "C" {
    fn getentropy(buf: *mut c_void, len: usize) -> std::ffi::c_int;
}

/// Fill `out` with random bytes.
#[cfg(target_vendor = "apple")]
pub(crate) fn fill(out: &mut [u8]) {
    // SAFETY: a writable buffer of that length; the call cannot fail
    unsafe { arc4random_buf(out.as_mut_ptr().cast(), out.len()) }
}

/// Fill `out` with random bytes, 256 at a time, getentropy's limit.
#[cfg(not(target_vendor = "apple"))]
pub(crate) fn fill(out: &mut [u8]) {
    for chunk in out.chunks_mut(256) {
        // SAFETY: a writable buffer of at most 256 bytes
        let r = unsafe { getentropy(chunk.as_mut_ptr().cast(), chunk.len()) };
        assert_eq!(r, 0, "the operating system's random source failed");
    }
}
