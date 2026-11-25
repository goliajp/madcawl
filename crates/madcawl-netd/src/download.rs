//! Downloads: a navigation's response with `Content-Disposition:
//! attachment` or of a type that is not shown as a page, or the response
//! to a fetch that asks for it, goes to a file in the profile's downloads
//! directory instead of BODY frames. Any other fetch gets its body, as
//! script's `fetch()` does, so a content process cannot have a file
//! written by fetching an attachment. The bytes go to `.<name>.part`,
//! which takes the final name only once the whole body is in and it is
//! marked as quarantined; a failed or cancelled download leaves nothing
//! behind. A final name never starts with a dot, so a leftover `.part` is
//! never a finished file. A file is capped at `per_file` bytes, and the
//! directory at `total` bytes and `max_files` entries across all
//! downloads at once.

mod budget;
mod name;
mod quarantine;

pub use name::{is_attachment, renders, sanitize, suggested};

use crate::{Writer, send};
use budget::Budget;
use madcawl_net::fetch::Sink;
use madcawl_net::sock::Cancel;
use madcawl_net::wire::{Frame, kind, tag};
use madcawl_net::{NetError, Response};
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// tries at a free numbered name before the download fails
const MAX_TRIES: usize = 1000;
/// entries read when removing leftovers
const MAX_ENTRIES: usize = 100_000;
/// bytes between two DOWNLOAD_PROGRESS frames
const PROGRESS_STEP: u64 = 1 << 20;

pub struct Downloads {
    pub dir: PathBuf,
    per_file: u64,
    total: u64,
    max_files: u64,
    budget: OnceLock<Budget>,
}

impl Downloads {
    pub fn new(dir: PathBuf) -> Self {
        Downloads::with_caps(dir, 2 << 30, 16 << 30, 10_000)
    }

    pub fn with_caps(dir: PathBuf, per_file: u64, total: u64, max_files: u64) -> Self {
        Downloads { dir, per_file, total, max_files, budget: OnceLock::new() }
    }

    fn budget(&self) -> &Budget {
        self.budget.get_or_init(|| Budget::measure(&self.dir, self.total, self.max_files))
    }

    /// Takes a free name for `wanted`: neither it nor its `.part` exists,
    /// and the `.part` is created here.
    fn reserve(&self, wanted: &str) -> io::Result<(String, PathBuf, File)> {
        for k in 0..MAX_TRIES {
            let n = name::numbered(wanted, k);
            if self.dir.join(n.as_ref()).exists() {
                continue;
            }
            let part = self.dir.join(format!(".{n}.part"));
            match OpenOptions::new().write(true).create_new(true).mode(0o600).open(&part) {
                Ok(f) => return Ok((n.into_owned(), part, f)),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(ErrorKind::AlreadyExists, "no free name"))
    }

    /// Gives the finished `.part` its name, or the next free numbered one
    /// if another file took it meanwhile; a hard link never replaces a
    /// file.
    fn publish(&self, wanted: &str, part: &Path) -> io::Result<String> {
        for k in 0..MAX_TRIES {
            let n = name::numbered(wanted, k);
            match std::fs::hard_link(part, self.dir.join(n.as_ref())) {
                Ok(()) => {
                    std::fs::remove_file(part)?;
                    return Ok(n.into_owned());
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(ErrorKind::AlreadyExists, "no free name"))
    }

    /// Removes the `.part` files a stopped process left; only while
    /// nothing else downloads into the directory.
    pub(crate) fn remove_partial(&self) {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return };
        for e in rd.take(MAX_ENTRIES).flatten() {
            let n = e.file_name();
            let n = n.to_string_lossy();
            if n.starts_with('.') && n.ends_with(".part") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

struct Active {
    name: String,
    part: PathBuf,
    file: File,
    bytes: u64,
    total: Option<u64>,
    reported: u64,
}

/// A fetch's sink that hands the response on, or saves it as a download.
pub(crate) struct Tee<'a> {
    pub inner: &'a mut dyn Sink,
    pub w: &'a Writer,
    pub id: u64,
    pub cancel: &'a Cancel,
    pub downloads: Option<&'a Downloads>,
    pub navigation: bool,
    pub forced: bool,
    active: Option<Active>,
}

fn io_error(what: &str, e: io::Error) -> NetError {
    NetError::Io(format!("{what}: {e}"))
}

impl<'a> Tee<'a> {
    pub fn new(inner: &'a mut dyn Sink, w: &'a Writer, id: u64, cancel: &'a Cancel, downloads: Option<&'a Downloads>) -> Self {
        Tee { inner, w, id, cancel, downloads, navigation: false, forced: false, active: None }
    }

    fn wanted(&self, r: &Response) -> bool {
        let disposition = r.header("content-disposition");
        (200..300).contains(&r.status)
            && (self.forced || (self.navigation && (disposition.is_some_and(is_attachment) || !renders(r.header("content-type")))))
    }

    fn start(&mut self, d: &Downloads, r: &Response) -> Result<(), NetError> {
        let total = r.header("content-length").and_then(|v| std::str::from_utf8(v).ok()?.trim().parse::<u64>().ok());
        if total.is_some_and(|t| t > d.per_file.min(d.budget().bytes_left())) {
            return Err(NetError::Refused("download over its size cap".into()));
        }
        if !d.budget().take_file() {
            return Err(NetError::Refused("the downloads directory holds too many files".into()));
        }
        let wanted = sanitize(&suggested(r.header("content-disposition"), r.url()));
        let (name, part, file) = d.reserve(&wanted).map_err(|e| {
            d.budget().give_back(0);
            io_error("download file", e)
        })?;
        let mut f = Frame::new(kind::DOWNLOAD).num(tag::ID, self.id).str(tag::FILE, &name);
        if let Some(t) = total {
            f = f.num(tag::TOTAL, t);
        }
        // the reserved name; publishing numbers it again only if a file took it meanwhile
        self.active = Some(Active { name, part, file, bytes: 0, total, reported: 0 });
        send(self.w, &f).map_err(|e| io_error("to the content process", e))
    }

    /// Ends the fetch: a finished download is quarantined and gets its
    /// name and a DOWNLOAD_DONE frame, a failed one loses its `.part` and
    /// gives back what it took of the directory. True when the response
    /// went to a file.
    pub fn finish(mut self, r: Result<(), NetError>) -> Result<bool, NetError> {
        let Some(a) = self.active.take() else { return r.map(|()| false) };
        let d = self.downloads.expect("a download has a directory");
        let done = r.and_then(|()| {
            quarantine::mark(&a.file, madcawl_net::fetch::now_secs() as u64).map_err(|e| io_error("download file", e))?;
            a.file.sync_all().map_err(|e| io_error("download file", e))?;
            d.publish(&a.name, &a.part).map_err(|e| io_error("download file", e))
        });
        match done {
            Ok(name) => {
                let f = Frame::new(kind::DOWNLOAD_DONE).num(tag::ID, self.id).str(tag::FILE, &name).num(tag::BYTES, a.bytes);
                send(self.w, &f).map_err(|e| io_error("to the content process", e))?;
                Ok(true)
            }
            Err(e) => {
                if std::fs::remove_file(&a.part).is_ok() {
                    d.budget().give_back(a.bytes);
                }
                Err(e)
            }
        }
    }
}

impl Sink for Tee<'_> {
    fn head(&mut self, r: &Response) -> Result<(), NetError> {
        self.inner.head(r)?;
        match self.downloads {
            Some(d) if self.wanted(r) => self.start(d, r),
            _ => Ok(()),
        }
    }

    fn body(&mut self, chunk: &[u8]) -> Result<(), NetError> {
        let Some(a) = &mut self.active else { return self.inner.body(chunk) };
        if self.cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        let d = self.downloads.expect("a download has a directory");
        let n = chunk.len() as u64;
        if a.bytes + n > d.per_file || !d.budget().take_bytes(n) {
            return Err(NetError::Refused("download over its size cap".into()));
        }
        a.bytes += n;
        a.file.write_all(chunk).map_err(|e| io_error("download file", e))?;
        if a.bytes - a.reported >= PROGRESS_STEP {
            a.reported = a.bytes;
            let mut f = Frame::new(kind::DOWNLOAD_PROGRESS).num(tag::ID, self.id).num(tag::BYTES, a.bytes);
            if let Some(t) = a.total {
                f = f.num(tag::TOTAL, t);
            }
            send(self.w, &f).map_err(|e| io_error("to the content process", e))?;
        }
        Ok(())
    }
}
