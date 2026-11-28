//! A test display end, plain Rust with no window: a host started as its
//! own process with `--isolate`, a connection playing the display end,
//! and a pane's display channel with shared-memory buffers it maps.
#![allow(dead_code)]

pub mod png;

use std::os::fd::{AsFd as _, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use madcawl_wire::display::{self, Buffer, Commit, Surface};
use madcawl_wire::host::{self, PaneOpen, PaneOpened, role};
use madcawl_wire::os::{self, Mapping};
use madcawl_wire::shm::Header;
use madcawl_wire::{Frame, FrameError, Hello, Outcome};

pub const EXE: &str = env!("CARGO_BIN_EXE_madcawl-shell");

pub const PAGE: &str = r#"<!doctype html><title>Pane</title>
<main><h1>Vec in std</h1>
<p style="color:rgb(160,20,40)">zebra-secret-paragraph lives here.</p>
<pre><code>pub fn push(&amp;mut self, value: T)</code></pre>
<a href="other.html">Other page</a>
<div style="width:200px;height:120px;background:rgb(10,120,250);border:4px solid rgb(250,200,0)"></div>
</main>"#;

/// A host process on a socket in a fresh directory holding [`PAGE`].
pub struct Host {
    child: Child,
    pub dir: PathBuf,
    pub socket: PathBuf,
}

impl Host {
    pub fn start(name: &str, extra: &[&str]) -> Host {
        let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mcw-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("page.html"), PAGE).unwrap();
        let socket = dir.join("s");
        let child = Command::new(EXE)
            .args(["--serve", socket.to_str().unwrap(), "--allow", dir.to_str().unwrap(), "--isolate"])
            .args(extra)
            .spawn()
            .unwrap();
        let h = Host { child, dir, socket };
        let deadline = Instant::now() + Duration::from_secs(10);
        while UnixStream::connect(&h.socket).is_err() {
            assert!(Instant::now() < deadline, "the host listens");
            std::thread::sleep(Duration::from_millis(10));
        }
        h
    }

    pub fn pid(&self) -> i32 {
        self.child.id() as i32
    }

    /// Whether the host process has exited within `secs`.
    pub fn exits_within(&mut self, secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if self.child.try_wait().unwrap().is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    pub fn page(&self) -> String {
        self.dir.join("page.html").display().to_string()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        // the host is this test's own child; its content and network
        // processes end with it
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

/// Whether process `pid` is still there.
pub fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists
    unsafe { kill(pid, 0) == 0 }
}

/// Frames on a socket, with the descriptors that come with them.
pub struct Link {
    pub s: UnixStream,
    buf: Vec<u8>,
    pub fds: Vec<OwnedFd>,
}

impl Link {
    pub fn new(s: UnixStream) -> Link {
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        Link { s, buf: Vec::new(), fds: Vec::new() }
    }

    pub fn send(&self, f: &Frame) {
        self.send_fds(f, &[]);
    }

    pub fn send_fds(&self, f: &Frame, fds: &[std::os::fd::BorrowedFd]) {
        let mut out = Vec::new();
        f.encode(&mut out);
        os::send(&self.s, &out, fds).unwrap();
    }

    pub fn next(&mut self) -> Frame {
        loop {
            match Frame::decode(&self.buf) {
                Ok((f, used)) => {
                    self.buf.drain(..used);
                    return f;
                }
                Err(FrameError::Short) => {}
                Err(e) => panic!("a bad frame: {e:?}"),
            }
            let mut chunk = [0u8; 4096];
            let n = os::recv(&self.s, &mut chunk, &mut self.fds).unwrap();
            assert!(n > 0, "closed before a frame");
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    /// Says hello as `role` on channel kind `kind` with `range`; what came
    /// of it.
    pub fn hello(&mut self, kind: u16, range: madcawl_wire::Range, role: Option<u32>) -> Hello {
        self.send(&Hello { range, role, version: None }.frame(kind));
        let f = self.next();
        assert_eq!(f.kind, kind, "{f:?}");
        Hello::from_frame(&f).unwrap()
    }
}

/// A connection playing the display end, past its hello.
pub fn display_end(h: &Host) -> Link {
    let mut l = Link::new(UnixStream::connect(&h.socket).unwrap());
    let a = l.hello(host::kind::HELLO, host::RANGE, Some(role::DISPLAY));
    assert_eq!(host::RANGE.negotiate(a.range), Outcome::Agreed(1));
    l
}

/// Opens `url` in a pane of `w` by `h` CSS px at `scale` hundredths;
/// what the host said and the display channel, past its hello.
pub fn open_pane(l: &mut Link, url: &str, (w, h): (u32, u32), scale: u32) -> (PaneOpened, Link) {
    l.send(&PaneOpen { session: 9, url: url.into(), width: w, height: h, scale }.frame());
    let f = l.next();
    let opened = PaneOpened::from_frame(&f).unwrap_or_else(|e| panic!("{e}: {:?}", host::refusal(&f)));
    assert_eq!(l.fds.len(), 1, "the display channel comes with the answer");
    let mut ch = Link::new(UnixStream::from(l.fds.remove(0)));
    let a = ch.hello(display::kind::HELLO, display::RANGE, None);
    assert_eq!(a.version, Some(display::RANGE.cur));
    (opened, ch)
}

/// A pane's surface as the display end holds it.
pub struct Pane {
    pub ch: Link,
    pub surface: Surface,
    pub maps: Vec<Mapping>,
    pub last: u64,
}

/// Makes `n` buffers of `w` by `h` device pixels and sends them.
pub fn attach(ch: Link, px: (u32, u32), scale: u32, n: usize) -> Pane {
    attach_with(ch, px, scale, n, |_| {})
}

/// Every surface's epoch comes from here, so each one on a channel is
/// larger than the last.
static EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The same, `change` let at the buffers before they are sent.
pub fn attach_with(ch: Link, (w, h): (u32, u32), scale: u32, n: usize, change: impl FnOnce(&mut [Mapping])) -> Pane {
    let (mut fds, mut maps, mut buffers) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..n {
        let bytes = Header::needed(w, h);
        let fd = os::shm_create(bytes).unwrap();
        let mut m = Mapping::map(fd.as_fd(), bytes).unwrap();
        let head = Header::new(w, h, bytes);
        head.write(m.bytes_mut());
        buffers.push(Buffer { bytes, width: w, height: h, stride: head.stride });
        fds.push(fd);
        maps.push(m);
    }
    change(&mut maps);
    let epoch = EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let surface = Surface { epoch, scale, buffers };
    let borrowed: Vec<_> = fds.iter().map(|f| f.as_fd()).collect();
    ch.send_fds(&surface.frame(), &borrowed);
    Pane { ch, surface, maps, last: 0 }
}

impl Pane {
    /// The next commit, checked as the display end must check it.
    pub fn commit(&mut self, reading: Option<u32>) -> Commit {
        let f = self.ch.next();
        let c = Commit::from_frame(&f).unwrap();
        c.check(&self.surface, self.last, reading).unwrap();
        assert_eq!(self.maps[c.buffer as usize].generation(), c.seq, "the pixels are marked written first");
        self.last = c.seq;
        c
    }

    /// Device pixel `(x, y)` of buffer `i` as (b, g, r, a).
    pub fn pixel(&self, i: usize, x: usize, y: usize) -> [u8; 4] {
        let b = self.surface.buffers[i];
        let at = madcawl_wire::shm::HEADER_LEN + y * b.stride as usize + x * 4;
        self.maps[i].bytes()[at..at + 4].try_into().unwrap()
    }
}
