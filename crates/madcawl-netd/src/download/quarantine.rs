//! The `com.apple.quarantine` attribute a downloaded file carries, so that
//! Gatekeeper checks it before it is opened. The value is the one browsers
//! write: flags, the download time in hex seconds and the agent's name,
//! separated by `;`, without the event id, since no quarantine event is
//! recorded for it.

use std::fs::File;
use std::io;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn fsetxattr(fd: i32, name: *const u8, value: *const u8, size: usize, position: u32, options: i32) -> i32;
}

/// the flags Safari writes on its downloads
#[cfg(target_os = "macos")]
const FLAGS: &str = "0083";
#[cfg(target_os = "macos")]
const AGENT: &str = "madcawl";

#[cfg(target_os = "macos")]
pub(super) fn mark(f: &File, now: u64) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let v = format!("{FLAGS};{now:08x};{AGENT};");
    // SAFETY: the name is nul-terminated and the value is valid for its length
    let r = unsafe { fsetxattr(f.as_raw_fd(), c"com.apple.quarantine".as_ptr().cast(), v.as_ptr(), v.len(), 0, 0) };
    if r == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn mark(_: &File, _: u64) -> io::Result<()> {
    Ok(())
}
