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
    /// The room left in `dir` under `total` bytes and `max_files` entries,
    /// subdirectories' included (a host moves finished downloads into
    /// them); a tree with more entries than that, which is read no
    /// further, counts as full. Links are not followed.
    pub(super) fn measure(dir: &Path, total: u64, max_files: u64) -> Budget {
        let (mut used, mut entries) = (0u64, 0u64);
        let mut dirs = vec![dir.to_path_buf()];
        while let Some(d) = dirs.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                entries += 1;
                if entries > max_files {
                    return Budget { bytes: AtomicU64::new(0), files: AtomicU64::new(0) };
                }
                match e.file_type() {
                    Ok(t) if t.is_dir() => dirs.push(e.path()),
                    Ok(t) if t.is_file() => used = used.saturating_add(e.metadata().map_or(0, |m| m.len())),
                    _ => {}
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

#[cfg(test)]
mod tests {
    use super::Budget;

    #[test]
    fn files_moved_into_subdirectories_still_count() {
        let dir = std::env::temp_dir().join(format!("madcawl-budget-{}", std::process::id()));
        let deep = dir.join("s1-a").join("x");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(dir.join("top.bin"), [0u8; 100]).unwrap();
        std::fs::write(deep.join("moved.bin"), [0u8; 300]).unwrap();
        let b = Budget::measure(&dir, 1000, 10);
        assert_eq!(b.bytes_left(), 600);
        assert_eq!(b.files.load(std::sync::atomic::Ordering::SeqCst), 10 - 4, "two files and two directories");
        assert_eq!(Budget::measure(&dir, 1000, 3).bytes_left(), 0, "more entries than allowed is full");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
