//! Shared memory and descriptors passed over a Unix socket.
//!
//! A buffer is a POSIX shared-memory object, unlinked as soon as it is
//! made, so it lives only as long as some process holds it. macOS sizes
//! such an object once: after the maker sets its size, no holder can
//! shrink it under another's mapping.

use std::fs::File;
use std::io::{self, IoSlice, Write as _};
use std::mem::ManuallyDrop;
use std::os::fd::{AsRawFd as _, BorrowedFd, FromRawFd as _, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::shm::{GENERATION_AT, HEADER_LEN};

/// The most descriptors one message carries.
pub const MAX_FDS: usize = 4;

#[repr(C)]
struct MsgHdr {
    name: *mut u8,
    namelen: u32,
    iov: *mut core::ffi::c_void,
    #[cfg(target_os = "macos")]
    iovlen: i32,
    #[cfg(not(target_os = "macos"))]
    iovlen: usize,
    control: *mut u8,
    #[cfg(target_os = "macos")]
    controllen: u32,
    #[cfg(not(target_os = "macos"))]
    controllen: usize,
    flags: i32,
}

#[cfg(target_os = "macos")]
type CmsgLen = u32;
#[cfg(not(target_os = "macos"))]
type CmsgLen = usize;

#[cfg(target_os = "macos")]
mod c {
    pub const SOL_SOCKET: i32 = 0xffff;
    pub const O_RDWR: i32 = 2;
    pub const O_CREAT: i32 = 0x200;
    pub const O_EXCL: i32 = 0x800;
    pub const MSG_CTRUNC: i32 = 0x20;
}
#[cfg(not(target_os = "macos"))]
mod c {
    pub const SOL_SOCKET: i32 = 1;
    pub const O_RDWR: i32 = 2;
    pub const O_CREAT: i32 = 0o100;
    pub const O_EXCL: i32 = 0o200;
    pub const MSG_CTRUNC: i32 = 0x8;
}
const SCM_RIGHTS: i32 = 1;
const F_SETFD: i32 = 2;
const FD_CLOEXEC: i32 = 1;
const PROT_READ: i32 = 1;
const PROT_WRITE: i32 = 2;
const MAP_SHARED: i32 = 1;

unsafe extern "C" {
    fn sendmsg(fd: i32, msg: *const MsgHdr, flags: i32) -> isize;
    fn recvmsg(fd: i32, msg: *mut MsgHdr, flags: i32) -> isize;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    fn shm_open(name: *const u8, oflag: i32, ...) -> i32;
    fn shm_unlink(name: *const u8) -> i32;
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    fn munmap(addr: *mut u8, len: usize) -> i32;
}

const fn align(n: usize) -> usize {
    let a = std::mem::size_of::<CmsgLen>();
    (n + a - 1) & !(a - 1)
}

/// The control header's length, padded: where a message's descriptors
/// start.
const HDR: usize = align(std::mem::size_of::<CmsgLen>() + 8);
const CONTROL: usize = HDR + align(MAX_FDS * 4);

/// Writes all of `bytes`, `fds` going with the first of them.
pub fn send(s: &UnixStream, bytes: &[u8], fds: &[BorrowedFd]) -> io::Result<()> {
    assert!(fds.len() <= MAX_FDS && !bytes.is_empty());
    let mut control = [0u8; CONTROL];
    let len = HDR + fds.len() * 4;
    control[..std::mem::size_of::<CmsgLen>()].copy_from_slice(&(len as CmsgLen).to_ne_bytes());
    let at = std::mem::size_of::<CmsgLen>();
    control[at..at + 4].copy_from_slice(&c::SOL_SOCKET.to_ne_bytes());
    control[at + 4..at + 8].copy_from_slice(&SCM_RIGHTS.to_ne_bytes());
    for (i, fd) in fds.iter().enumerate() {
        control[HDR + i * 4..HDR + i * 4 + 4].copy_from_slice(&fd.as_raw_fd().to_ne_bytes());
    }
    let mut iov = [IoSlice::new(bytes)];
    let msg = MsgHdr {
        name: std::ptr::null_mut(),
        namelen: 0,
        iov: iov.as_mut_ptr().cast(),
        iovlen: 1,
        control: if fds.is_empty() { std::ptr::null_mut() } else { control.as_mut_ptr() },
        controllen: if fds.is_empty() { 0 } else { align(len) as _ },
        flags: 0,
    };
    let sent = loop {
        // SAFETY: the header, its one buffer and the control bytes live for the call
        let n = unsafe { sendmsg(s.as_raw_fd(), &msg, 0) };
        if n >= 0 {
            break n as usize;
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    };
    (&*s).write_all(&bytes[sent..])
}

/// Reads what is there, up to `buf`'s length, into `buf`; descriptors
/// that came with it go to `fds`. 0 is the end of the stream. More
/// descriptors than [`MAX_FDS`] in one message is an error, and those
/// that came are closed.
///
/// Close-on-exec is set on each descriptor after `recvmsg` returns, as
/// macOS has no `MSG_CMSG_CLOEXEC`: a child spawned by another thread
/// in between inherits it. A receiver with threads that spawn children
/// receives before they may spawn, or spawns under a lock it shares
/// with this call.
pub fn recv(s: &UnixStream, buf: &mut [u8], fds: &mut Vec<OwnedFd>) -> io::Result<usize> {
    let mut control = [0u8; CONTROL];
    let mut iov = [io::IoSliceMut::new(buf)];
    let mut msg = MsgHdr {
        name: std::ptr::null_mut(),
        namelen: 0,
        iov: iov.as_mut_ptr().cast(),
        iovlen: 1,
        control: control.as_mut_ptr(),
        controllen: CONTROL as _,
        flags: 0,
    };
    let n = loop {
        // SAFETY: the header, its one buffer and the control bytes live for the call
        let n = unsafe { recvmsg(s.as_raw_fd(), &mut msg, 0) };
        if n >= 0 {
            break n as usize;
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    };
    let got = take_fds(&control[..msg.controllen as usize])?;
    if msg.flags & c::MSG_CTRUNC != 0 {
        return Err(io::Error::other("more descriptors than a message may carry"));
    }
    fds.extend(got);
    Ok(n)
}

/// The descriptors in the control messages `b` holds, each owned and
/// closed on exec.
fn take_fds(mut b: &[u8]) -> io::Result<Vec<OwnedFd>> {
    let mut out = Vec::new();
    let w = std::mem::size_of::<CmsgLen>();
    while b.len() >= HDR {
        let len = CmsgLen::from_ne_bytes(b[..w].try_into().expect("a length")) as usize;
        let level = i32::from_ne_bytes(b[w..w + 4].try_into().expect("four bytes"));
        let kind = i32::from_ne_bytes(b[w + 4..w + 8].try_into().expect("four bytes"));
        if len < HDR || len > b.len() {
            return Err(io::Error::other("a malformed control message"));
        }
        if level == c::SOL_SOCKET && kind == SCM_RIGHTS {
            for raw in b[HDR..len].as_chunks::<4>().0 {
                let fd = i32::from_ne_bytes(*raw);
                // SAFETY: the kernel made this descriptor for this process just now
                let owned = unsafe { OwnedFd::from_raw_fd(fd) };
                // SAFETY: fcntl on a descriptor this process owns
                unsafe { fcntl(fd, F_SETFD, FD_CLOEXEC) };
                out.push(owned);
            }
        }
        b = &b[align(len).min(b.len())..];
    }
    Ok(out)
}

/// The size of the object `fd` refers to.
pub fn size_of(fd: BorrowedFd) -> io::Result<u64> {
    // SAFETY: borrowed only to read its size; ManuallyDrop keeps it open
    let f = ManuallyDrop::new(unsafe { File::from_raw_fd(fd.as_raw_fd()) });
    Ok(f.metadata()?.len())
}

/// A new shared-memory object of `len` bytes, zeroed, reachable only
/// through the descriptor.
pub fn shm_create(len: u64) -> io::Result<OwnedFd> {
    static N: AtomicU32 = AtomicU32::new(0);
    // a short name: macOS takes at most 31 bytes
    let name = format!("/mcw{}.{}\0", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
    // SAFETY: a nul-terminated name; the mode is passed as the variadic third argument
    let fd = unsafe { shm_open(name.as_ptr(), c::O_RDWR | c::O_CREAT | c::O_EXCL, 0o600_u32) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: as above; the object stays while the descriptor does
    unsafe { shm_unlink(name.as_ptr()) };
    // SAFETY: shm_open returned a descriptor this process owns
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: fcntl on a descriptor this process owns
    unsafe { fcntl(fd, F_SETFD, FD_CLOEXEC) };
    File::from(owned.try_clone()?).set_len(len)?;
    Ok(owned)
}

/// A shared mapping of a buffer, unmapped when dropped.
pub struct Mapping {
    ptr: *mut u8,
    len: usize,
}

// SAFETY: the mapping is plain memory; who writes it when is the
// protocol's business, and the generation is read and written atomically
unsafe impl Send for Mapping {}

impl Mapping {
    /// Maps the first `len` bytes of `fd` to read and write, once its
    /// object is found to hold them: touching memory past the object's
    /// end would end the process.
    pub fn map(fd: BorrowedFd, len: u64) -> io::Result<Mapping> {
        if len < HEADER_LEN as u64 || size_of(fd)? < len {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "the buffer is smaller than it is said to be"));
        }
        let len = usize::try_from(len).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let raw: RawFd = fd.as_raw_fd();
        // SAFETY: a fresh shared mapping of a range the object holds
        let ptr = unsafe { mmap(std::ptr::null_mut(), len, PROT_READ | PROT_WRITE, MAP_SHARED, raw, 0) };
        if ptr as isize == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(Mapping { ptr, len })
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.ptr
    }

    pub fn bytes(&self) -> &[u8] {
        // SAFETY: `len` mapped bytes for as long as `self` lives
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as above, borrowed mutably through `self`
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    fn generation_cell(&self) -> &AtomicU64 {
        // SAFETY: the mapping is page-aligned and at least a header long,
        // so the generation's offset is an aligned u64 within it
        unsafe { &*self.ptr.add(GENERATION_AT).cast::<AtomicU64>() }
    }

    /// The commit sequence the pixels were last written for.
    pub fn generation(&self) -> u64 {
        u64::from_le(self.generation_cell().load(Ordering::Acquire))
    }

    /// Marks the pixels written for commit `seq`, after they are.
    pub fn set_generation(&self, seq: u64) {
        self.generation_cell().store(seq.to_le(), Ordering::Release);
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: the range this mapping made
        unsafe { munmap(self.ptr, self.len) };
    }
}

#[cfg(test)]
#[path = "os_tests.rs"]
mod tests;
