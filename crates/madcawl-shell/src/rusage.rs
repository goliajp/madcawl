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

/// `rusage_info_v6`: a 16-byte uuid, then u64 counters; the ones read
/// here are named by their index after the uuid.
#[repr(C)]
struct RusageInfoV6 {
    uuid: [u8; 16],
    f: [u64; 56],
}

const USER_TIME: usize = 0;
const SYSTEM_TIME: usize = 1;
const PKG_IDLE_WKUPS: usize = 2;
const INTERRUPT_WKUPS: usize = 3;
const PHYS_FOOTPRINT: usize = 7;
const LIFETIME_MAX_FOOTPRINT: usize = 28;
const ENERGY_NJ: usize = 40;

#[repr(C)]
struct Timebase {
    numer: u32,
    denom: u32,
}

unsafe extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV6) -> i32;
    fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buffer: *mut u8, size: i32) -> i32;
    fn mach_timebase_info(info: *mut Timebase) -> i32;
    fn sysctlbyname(name: *const u8, old: *mut u8, oldlen: *mut usize, new: *const u8, newlen: usize) -> i32;
    fn getpid() -> i32;
}

/// What the system counts against one process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// User plus system CPU time
    pub cpu_ns: u64,
    /// What Activity Monitor shows as Memory
    pub footprint: u64,
    pub lifetime_max_footprint: u64,
    /// Wakeups from interrupts and timers; on Apple silicon the idle
    /// package wakeup counter stays 0, so this is the one to read
    pub interrupt_wakeups: u64,
    pub idle_wakeups: u64,
    /// The energy the system has billed the process, in nanojoules
    pub energy_nj: u64,
}

/// The counters of process `pid`, which must be this user's.
pub fn usage_of(pid: i32) -> std::io::Result<Usage> {
    const RUSAGE_INFO_V6: i32 = 6;
    let mut r = RusageInfoV6 { uuid: [0; 16], f: [0; 56] };
    // SAFETY: the struct has rusage_info_v6's layout, a 16-byte uuid then 56 u64s, which the call fills
    if unsafe { proc_pid_rusage(pid, RUSAGE_INFO_V6, &mut r) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut tb = Timebase { numer: 1, denom: 1 };
    // SAFETY: fills the two u32s
    unsafe { mach_timebase_info(&mut tb) };
    let ticks = u128::from(r.f[USER_TIME] + r.f[SYSTEM_TIME]);
    Ok(Usage {
        cpu_ns: (ticks * u128::from(tb.numer) / u128::from(tb.denom.max(1))) as u64,
        footprint: r.f[PHYS_FOOTPRINT],
        lifetime_max_footprint: r.f[LIFETIME_MAX_FOOTPRINT],
        interrupt_wakeups: r.f[INTERRUPT_WKUPS],
        idle_wakeups: r.f[PKG_IDLE_WKUPS],
        energy_nj: r.f[ENERGY_NJ],
    })
}

/// This process's physical footprint (what Activity Monitor shows as
/// Memory), in bytes.
pub fn footprint() -> u64 {
    // SAFETY: getpid has no preconditions
    usage_of(unsafe { getpid() }).expect("proc_pid_rusage of this process").footprint
}

/// How many file descriptors process `pid` has open.
pub fn fd_count(pid: i32) -> std::io::Result<usize> {
    fds(pid).map(|f| f.len())
}

/// The file descriptors process `pid` has open.
pub fn fds(pid: i32) -> std::io::Result<Vec<i32>> {
    const PROC_PIDLISTFDS: i32 = 1;
    // a proc_fdinfo is an i32 descriptor then a u32 type; the call with no buffer says how many bytes it would fill
    // SAFETY: a null buffer of size 0 only asks for the size
    let want = unsafe { proc_pidinfo(pid, PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    if want <= 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut buf = vec![0u8; want as usize + 64 * 8];
    // SAFETY: the buffer is as long as the size passed
    let got = unsafe { proc_pidinfo(pid, PROC_PIDLISTFDS, 0, buf.as_mut_ptr(), buf.len() as i32) };
    if got <= 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(buf[..got as usize].as_chunks::<8>().0.iter().map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

/// How many threads process `pid` has.
pub fn thread_count(pid: i32) -> std::io::Result<usize> {
    const PROC_PIDTASKINFO: i32 = 4;
    // proc_taskinfo is 96 bytes, pti_threadnum the i32 at 84
    let mut buf = [0u8; 96];
    // SAFETY: the buffer is as long as the size passed
    if unsafe { proc_pidinfo(pid, PROC_PIDTASKINFO, 0, buf.as_mut_ptr(), 96) } != 96 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(i32::from_le_bytes([buf[84], buf[85], buf[86], buf[87]]).max(0) as usize)
}

/// The machine's physical memory, in bytes.
pub fn physical_memory() -> u64 {
    let mut v = 0u64;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: hw.memsize is a u64, written into `v` of that size
    let r = unsafe { sysctlbyname(c"hw.memsize".as_ptr().cast(), (&raw mut v).cast(), &mut len, std::ptr::null(), 0) };
    assert_eq!(r, 0, "sysctl hw.memsize");
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_counted() {
        // SAFETY: getpid has no preconditions
        let pid = unsafe { getpid() };
        let a = usage_of(pid).unwrap();
        // a block larger than anything the other tests in this process hold
        let block = vec![7u8; 64 << 20];
        let b = usage_of(pid).unwrap();
        assert!(b.footprint >= a.footprint + (48 << 20), "the touched block is counted: {a:?} {b:?}");
        // one reply reads the footprint and its high-water mark at different
        // moments, so while another thread allocates the mark can trail the
        // footprint beside it; a later read never trails an earlier one
        let c = usage_of(pid).unwrap();
        assert!(c.lifetime_max_footprint >= b.footprint, "{b:?} {c:?}");
        // at least 4 instructions an iteration: a neighbouring counter
        // (instructions, cycles) would grow by more than 10^8
        let mut x = 0u64;
        for i in 0..25_000_000u64 {
            x = std::hint::black_box(x ^ i);
        }
        let d = usage_of(pid).unwrap();
        assert!(d.cpu_ns > a.cpu_ns && d.lifetime_max_footprint - c.lifetime_max_footprint < 100_000_000, "{x} {c:?} {d:?}");
        drop(block);
        assert!(fd_count(pid).unwrap() >= 3, "stdin, stdout and stderr at least");
        assert!(fds(pid).unwrap().starts_with(&[0, 1, 2]), "{:?}", fds(pid));
        assert!(thread_count(pid).unwrap() >= 1);
        assert!(physical_memory() >= 1 << 30);
        assert!(usage_of(-1).is_err());
    }
}
