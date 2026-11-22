//! The three memory calls the arena makes, declared here: the crate
//! depends on nothing but the standard library.

use std::ffi::{c_int, c_void};

unsafe extern "C" {
    fn mmap(
        addr: *mut c_void,
        len: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        off: i64,
    ) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> c_int;
    fn madvise(addr: *mut c_void, len: usize, advice: c_int) -> c_int;
    fn sysconf(name: c_int) -> std::ffi::c_long;
}

// the Linux values below are the generic ones, which these architectures
// use; others (mips, sparc, alpha, parisc) differ
#[cfg(all(
    target_os = "linux",
    not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64"
    ))
))]
compile_error!("the mmap constants are only written down for x86_64, aarch64 and riscv64");
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("the mmap constants are only written down for macOS and Linux");

const PROT_READ: c_int = 1;
const PROT_WRITE: c_int = 2;
const MAP_PRIVATE: c_int = 2;
#[cfg(target_os = "macos")]
const MAP_ANON: c_int = 0x1000;
#[cfg(target_os = "linux")]
const MAP_ANON: c_int = 0x20 | 0x4000; // MAP_ANONYMOUS | MAP_NORESERVE
#[cfg(target_os = "macos")]
const MADV_GIVE_BACK: c_int = 5; // MADV_FREE
#[cfg(target_os = "linux")]
const MADV_GIVE_BACK: c_int = 4; // MADV_DONTNEED
#[cfg(target_os = "macos")]
const SC_PAGESIZE: c_int = 29;
#[cfg(target_os = "linux")]
const SC_PAGESIZE: c_int = 30;

/// The system's page size: memory is given back in whole pages.
pub(crate) fn page_size() -> usize {
    // SAFETY: sysconf reads a constant
    let n = unsafe { sysconf(SC_PAGESIZE) };
    usize::try_from(n).expect("a page size")
}

/// Reserve `len` bytes aligned to `len` (a power of two): readable and
/// writable, backed by memory only where it is touched.
pub(crate) fn reserve_aligned(len: usize) -> *mut u8 {
    let span = 2 * len;
    // SAFETY: a fresh private anonymous mapping touches nothing else
    let p = unsafe {
        mmap(
            std::ptr::null_mut(),
            span,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANON,
            -1,
            0,
        )
    };
    if p as isize == -1 {
        panic!("cannot reserve {len} bytes of address space for the node arena");
    }
    let start = p as usize;
    let aligned = (start + len - 1) & !(len - 1);
    let (head, tail) = (aligned - start, start + span - (aligned + len));
    // SAFETY: both ranges are inside the mapping just made and outside
    // the part kept
    unsafe {
        if head > 0 {
            munmap(p, head);
        }
        if tail > 0 {
            munmap((aligned + len) as *mut c_void, tail);
        }
    }
    aligned as *mut u8
}

/// # Safety
/// `p` and `len` are what `reserve_aligned` gave and returned.
pub(crate) unsafe fn release(p: *mut u8, len: usize) {
    // SAFETY: the caller's
    unsafe { munmap(p.cast(), len) };
}

/// Let the system take back the memory behind `len` bytes at `p`; the
/// range reads as zeros or as what it held until it is written again.
///
/// # Safety
/// The range is inside a reservation, aligned to and a multiple of the
/// page size, and nothing is read from it until it has been written
/// again.
pub(crate) unsafe fn give_back(p: *mut u8, len: usize) {
    // SAFETY: the caller's
    unsafe { madvise(p.cast(), len, MADV_GIVE_BACK) };
}
