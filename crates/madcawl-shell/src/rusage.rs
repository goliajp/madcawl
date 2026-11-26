//! The process's peak resident memory and CPU time, from `getrusage`,
//! and its physical footprint, from macOS's `proc_pid_rusage` (the shell
//! draws with CoreGraphics, so it runs on macOS only).

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

fn usage() -> Rusage {
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
    u
}

/// The user and system CPU time this process has used so far.
pub fn cpu_time() -> std::time::Duration {
    let u = usage();
    let us = |t: &Timeval| t.sec as u64 * 1_000_000 + t.usec as u64;
    std::time::Duration::from_micros(us(&u.utime) + us(&u.stime))
}

/// Peak resident set size of this process so far, in bytes.
pub fn peak_rss() -> u64 {
    let u = usage();
    // macOS reports bytes, Linux kibibytes
    if cfg!(target_os = "macos") {
        u.maxrss as u64
    } else {
        u.maxrss as u64 * 1024
    }
}

#[repr(C)]
struct RusageInfoV2 {
    uuid: [u8; 16],
    // user and system time, idle and interrupt wakeups, pageins, wired size, resident size, phys_footprint, ...
    f: [u64; 18],
}

unsafe extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV2) -> i32;
    fn getpid() -> i32;
}

/// This process's physical footprint (what Activity Monitor shows as
/// Memory), in bytes.
pub fn footprint() -> u64 {
    const RUSAGE_INFO_V2: i32 = 2;
    let mut r = RusageInfoV2 {
        uuid: [0; 16],
        f: [0; 18],
    };
    // SAFETY: the struct has rusage_info_v2's layout, a 16-byte uuid then 18 u64s, which the call fills
    let rc = unsafe { proc_pid_rusage(getpid(), RUSAGE_INFO_V2, &mut r) };
    assert_eq!(rc, 0, "proc_pid_rusage of this process");
    r.f[7]
}
