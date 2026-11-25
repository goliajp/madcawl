//! What the downloads directory may still take, shared by every download
//! of one network process: bytes are taken as each chunk is written and
//! files as each download starts, and both are given back when a download
//! fails, so concurrent downloads together stay within the caps. The
//! directory is measured once, at the first download; files removed from
//! it while the process runs are not given back until the next start.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) struct Budget {
    bytes: AtomicU64,
    files: AtomicU64,
}

fn take(counter: &AtomicU64, n: u64) -> bool {
    counter.try_update(Ordering::SeqCst, Ordering::SeqCst, |left| left.checked_sub(n)).is_ok()
}

impl Budget {
    /// The room left in `dir` under `total` bytes and `max_files` entries;
    /// a directory with more entries than that, which is read no further,
    /// counts as full.
    pub(super) fn measure(dir: &Path, total: u64, max_files: u64) -> Budget {
        let (mut used, mut entries) = (0u64, 0u64);
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                entries += 1;
                if entries > max_files {
                    return Budget { bytes: AtomicU64::new(0), files: AtomicU64::new(0) };
                }
                if let Some(m) = e.metadata().ok().filter(|m| m.is_file()) {
                    used = used.saturating_add(m.len());
                }
            }
        }
        Budget { bytes: AtomicU64::new(total.saturating_sub(used)), files: AtomicU64::new(max_files - entries) }
    }

    pub(super) fn bytes_left(&self) -> u64 {
        self.bytes.load(Ordering::SeqCst)
    }

    pub(super) fn take_file(&self) -> bool {
        take(&self.files, 1)
    }

    pub(super) fn take_bytes(&self, n: u64) -> bool {
        take(&self.bytes, n)
    }

    /// Gives back what a download that left nothing behind took.
    pub(super) fn give_back(&self, bytes: u64) {
        self.bytes.fetch_add(bytes, Ordering::SeqCst);
        self.files.fetch_add(1, Ordering::SeqCst);
    }
}
