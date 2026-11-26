//! The host's handle on one content process: started with the channel
//! on its standard input, asked a frame at a time, and ended.

use std::ffi::OsString;
use std::io;
use std::os::fd::{AsRawFd as _, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use std::os::unix::process::ExitStatusExt as _;

use madcawl_agent::Frame;

use crate::serve::link::{internal, read_until, write};
use crate::serve::memory;

/// How long an ended session's content process may take to close its
/// pages before it is killed.
const FINISH_WAIT: Duration = Duration::from_secs(30);

pub(super) struct Content {
    child: Child,
    s: UnixStream,
    buf: Vec<u8>,
}

/// Why an answer did not come.
pub(super) enum Fault {
    /// The process ended; how
    Ended(String),
    /// Nothing came within the wait
    Hung,
}

/// What a content process's exit status says.
pub(super) fn describe(s: ExitStatus) -> String {
    match (s.code(), s.signal()) {
        (Some(memory::EXIT_CODE), _) => "it went over its memory cap".into(),
        (Some(c), _) => format!("it exited with status {c}"),
        (None, Some(sig)) => format!("it was ended by signal {sig}"),
        _ => format!("{s}"),
    }
}

impl Content {
    pub fn start(exe: &Path, args: &[OsString]) -> io::Result<Content> {
        let (host, theirs) = UnixStream::pair()?;
        let child = Command::new(exe).args(args).stdin(Stdio::from(OwnedFd::from(theirs))).stdout(Stdio::null()).spawn()?;
        Ok(Content { child, s: host, buf: Vec::new() })
    }

    pub fn pid(&self) -> i32 {
        self.child.id() as i32
    }

    pub fn fd(&self) -> RawFd {
        self.s.as_raw_fd()
    }

    /// The answer to `f`. Questions the process asks meanwhile are
    /// answered with `answer`.
    pub fn ask(&mut self, f: &Frame, wait: Duration, answer: &mut dyn FnMut(&Frame) -> Frame) -> Result<Frame, Fault> {
        let deadline = Instant::now() + wait;
        if write(&mut self.s, f).is_err() {
            return Err(Fault::Ended(self.reap()));
        }
        loop {
            match read_until(&mut self.s, &mut self.buf, deadline) {
                Ok(Some(q)) if internal::is_internal(q.kind) => {
                    if write(&mut self.s, &answer(&q)).is_err() {
                        return Err(Fault::Ended(self.reap()));
                    }
                }
                Ok(Some(r)) => return Ok(r),
                Ok(None) => return Err(Fault::Hung),
                Err(_) => return Err(Fault::Ended(self.reap())),
            }
        }
    }

    /// Reads what the process said while no request was open: `None` when
    /// it has ended, with how.
    pub fn idle_read(&mut self) -> Option<String> {
        match read_until(&mut self.s, &mut self.buf, Instant::now() + Duration::from_millis(50)) {
            Ok(_) => None,
            Err(_) => Some(self.reap()),
        }
    }

    /// Whether the process ended with success, which is how a content
    /// process ends a session left idle with no pages.
    pub fn ended_well(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_some_and(|s| s.success())
    }

    /// Waits for the process to end, killing it if it has not; how it
    /// ended.
    pub fn reap(&mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.child.try_wait() {
                Ok(Some(s)) => return describe(s),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
                _ => return self.kill(),
            }
        }
    }

    /// Kills the process, which this host started and is its own.
    pub fn kill(&mut self) -> String {
        let _ = self.child.kill();
        self.child.wait().map_or_else(|e| e.to_string(), describe)
    }

    /// Ends the session: the process closes its pages, asking what it
    /// needs on the way, and exits.
    pub fn finish(mut self, answer: &mut dyn FnMut(&Frame) -> Frame) {
        if write(&mut self.s, &Frame::new(internal::END)).is_err() {
            self.reap();
            return;
        }
        let deadline = Instant::now() + FINISH_WAIT;
        loop {
            match read_until(&mut self.s, &mut self.buf, deadline) {
                Ok(Some(q)) if internal::is_internal(q.kind) => {
                    let _ = write(&mut self.s, &answer(&q));
                }
                Ok(Some(_)) => {}
                Ok(None) => {
                    self.kill();
                    return;
                }
                Err(_) => {
                    self.reap();
                    return;
                }
            }
        }
    }
}
