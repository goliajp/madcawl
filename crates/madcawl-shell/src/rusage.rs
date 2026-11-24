//! The process's peak resident memory, from `getrusage`.

#[repr(C)]
struct Timeval {
    sec: i64,
    usec: i64,
}

#[repr(C)]
struct Rusage {
    utime: Timeval,
    stime: Timeval,
    maxrss: i64,
    rest: [i64; 13],
}

unsafe extern "C" {
    fn getrusage(who: i32, usage: *mut Rusage) -> i32;
}

const RUSAGE_SELF: i32 = 0;

/// Peak resident set size of this process so far, in bytes.
pub fn peak_rss() -> u64 {
    let mut u = Rusage {
        utime: Timeval { sec: 0, usec: 0 },
        stime: Timeval { sec: 0, usec: 0 },
        maxrss: 0,
        rest: [0; 13],
    };
    // getrusage only writes into the struct, which has the C layout on
    // both 64-bit targets (an 8-byte-aligned timeval is 16 bytes)
    let r = unsafe { getrusage(RUSAGE_SELF, &mut u) };
    assert_eq!(r, 0, "getrusage(RUSAGE_SELF) does not fail");
    // macOS reports bytes, Linux kibibytes
    if cfg!(target_os = "macos") {
        u.maxrss as u64
    } else {
        u.maxrss as u64 * 1024
    }
}
