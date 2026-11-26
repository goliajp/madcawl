//! A local page's linked stylesheets, each named by its file's identity
//! so that a sheet the host holds is reused without reading the file.
//!
//! The identity is the file's device, inode, size, change and
//! modification times, and path. Any write moves the change time, which
//! nothing can set back, so an unchanged identity means unchanged bytes,
//! with one exception: a write within the time the file system's clock
//! cannot tell apart from the last change. A read therefore vouches for
//! its identity only when the file was last changed `settle` before the
//! read began and did not change while it was read; otherwise the bytes
//! are compared next time as well (as git treats a racily clean index).
//!
//! That holds only where the change time is the local kernel's own: an
//! NFS or SMB server stamps it with its clock, and FUSE or FAT file
//! systems may keep none (a same-length rewrite with its times copied
//! back would look unchanged). So a read vouches for an identity only on
//! a local APFS or HFS+ volume; anywhere else every open reads the file
//! and the cache compares its text.

use std::borrow::Cow;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use madcawl_css::style::{Fetched, LinkedSheets};

use crate::load::{MAX_FILE, open_regular, read_open, sheet_path};

/// Longer than the coarsest file system clock a page may be on (FAT's
/// two seconds)
pub const SETTLE: Duration = Duration::from_secs(3);

/// The identity of the regular file `m` describes, at `path`.
pub fn file_identity(path: &Path, m: &std::fs::Metadata) -> Vec<u8> {
    let mut id = Vec::with_capacity(56 + path.as_os_str().len());
    for n in [
        m.dev() as i64,
        m.ino() as i64,
        m.size() as i64,
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    ] {
        id.extend_from_slice(&n.to_le_bytes());
    }
    id.extend_from_slice(path.as_os_str().as_bytes());
    id
}

fn nanos(secs: i64, nsec: i64) -> i128 {
    i128::from(secs) * 1_000_000_000 + i128::from(nsec)
}

/// The file systems whose change times an identity may rest on.
fn keeps_change_time(flags: u32, kind: &[u8]) -> bool {
    const MNT_LOCAL: u32 = 0x1000;
    let name = kind.split(|&b| b == 0).next().unwrap_or(&[]);
    flags & MNT_LOCAL != 0 && matches!(name, b"apfs" | b"hfs")
}

#[cfg(target_os = "macos")]
mod volume {
    /// `struct statfs` with 64-bit inodes
    #[repr(C)]
    pub(super) struct Statfs {
        bsize: u32,
        iosize: i32,
        counts: [u64; 5],
        fsid: [i32; 2],
        owner: u32,
        kind: u32,
        pub flags: u32,
        subtype: u32,
        pub type_name: [u8; 16],
        on: [u8; 1024],
        from: [u8; 1024],
        flags_ext: u32,
        reserved: [u32; 7],
    }

    const _: () = assert!(size_of::<Statfs>() == 2168);

    unsafe extern "C" {
        #[cfg_attr(target_arch = "x86_64", link_name = "fstatfs$INODE64")]
        fn fstatfs(fd: i32, buf: *mut Statfs) -> i32;
    }

    pub(super) fn of(f: &std::fs::File) -> Option<Statfs> {
        use std::os::fd::AsRawFd as _;
        let mut s = Statfs {
            bsize: 0,
            iosize: 0,
            counts: [0; 5],
            fsid: [0; 2],
            owner: 0,
            kind: 0,
            flags: 0,
            subtype: 0,
            type_name: [0; 16],
            on: [0; 1024],
            from: [0; 1024],
            flags_ext: 0,
            reserved: [0; 7],
        };
        // fstatfs only writes into the struct, which has the C layout
        let r = unsafe { fstatfs(f.as_raw_fd(), &mut s) };
        (r == 0).then_some(s)
    }
}

/// Whether `f` is on a volume whose change times an identity may rest on.
#[cfg(target_os = "macos")]
fn on_trusted_volume(f: &std::fs::File) -> bool {
    volume::of(f).is_some_and(|s| keeps_change_time(s.flags, &s.type_name))
}

#[cfg(not(target_os = "macos"))]
fn on_trusted_volume(_: &std::fs::File) -> bool {
    false
}

/// The bytes of the regular file at `path` (as `load::read_file`), and
/// its identity when the read vouches for it: the file is on a local
/// APFS or HFS+ volume, did not change while read and was last changed
/// `settle` before.
pub fn read_known(
    path: &Path,
    max: u64,
    settle: Duration,
) -> std::io::Result<(Vec<u8>, Option<Vec<u8>>)> {
    let started = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i128);
    let f = open_regular(path)?;
    let before = f.metadata()?;
    let bytes = read_open(&f, max)?;
    let after = f.metadata()?;
    let id = file_identity(path, &before);
    let last =
        nanos(before.ctime(), before.ctime_nsec()).max(nanos(before.mtime(), before.mtime_nsec()));
    let settled = last + (settle.as_nanos() as i128) < started;
    let vouched = settled && id == file_identity(path, &after) && on_trusted_volume(&f);
    Ok((bytes, vouched.then_some(id)))
}

/// The stylesheets of a page whose file is in `base`, from files below
/// `allow` (any when `None`).
pub struct FileSheets<'a> {
    base: Option<&'a Path>,
    allow: Option<&'a [PathBuf]>,
    settle: Duration,
    /// How many files were read
    pub reads: u32,
}

impl<'a> FileSheets<'a> {
    pub fn new(base: Option<&'a Path>, allow: Option<&'a [PathBuf]>) -> FileSheets<'a> {
        FileSheets::settling(base, allow, SETTLE)
    }

    /// The same, a file's identity vouched for once it has not changed for `settle`.
    pub fn settling(
        base: Option<&'a Path>,
        allow: Option<&'a [PathBuf]>,
        settle: Duration,
    ) -> FileSheets<'a> {
        FileSheets {
            base,
            allow,
            settle,
            reads: 0,
        }
    }

    fn path(&self, href: &str) -> Option<PathBuf> {
        sheet_path(self.base?, href, self.allow)
    }
}

impl LinkedSheets for FileSheets<'_> {
    fn identity(&mut self, href: &str) -> Option<Vec<u8>> {
        let p = self.path(href)?;
        let m = std::fs::metadata(&p)
            .ok()
            .filter(std::fs::Metadata::is_file)?;
        Some(file_identity(&p, &m))
    }

    fn fetch(&mut self, href: &str) -> Option<Fetched<'_>> {
        let p = self.path(href)?;
        self.reads += 1;
        let (bytes, identity) = read_known(&p, MAX_FILE, self.settle).ok()?;
        Some(Fetched {
            bytes: Cow::Owned(bytes),
            identity,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use madcawl_css::style::{
        Device, Scope, SheetCache, Styles, style_document, style_document_cached,
    };
    use madcawl_dom::{Dom, NodeKind};
    use madcawl_html::Encoding;

    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Dir {
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::SeqCst);
            let d = std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(format!("mcs-sheets-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&d).unwrap();
            Dir(d)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const SCOPE: Scope = Scope {
        partition: 1,
        site: "file:",
    };

    fn style(dom: &Dom, links: &mut FileSheets, cache: &SheetCache) -> Styles {
        style_document_cached(dom, Device::default(), Encoding::Utf8, links, cache, SCOPE)
    }

    fn same(dom: &Dom, a: &Styles, b: &Styles) -> bool {
        dom.descendants(dom.document())
            .filter(|&n| dom.kind(n) == NodeKind::Element)
            .all(|n| a.get(n) == b.get(n))
    }

    #[test]
    fn a_settled_read_is_named_by_the_file() {
        let d = Dir::new();
        let p = d.0.join("a.css");
        std::fs::write(&p, "p { color: red }").unwrap();
        let (bytes, id) = read_known(&p, MAX_FILE, Duration::ZERO).unwrap();
        assert_eq!(bytes, b"p { color: red }");
        assert_eq!(
            id.unwrap(),
            file_identity(&p, &std::fs::metadata(&p).unwrap())
        );
        // just written: the clock may not tell a write now from the last one
        assert_eq!(read_known(&p, MAX_FILE, SETTLE).unwrap().1, None);
    }

    #[test]
    fn only_local_apfs_and_hfs_volumes_are_trusted() {
        let name = |s: &str| {
            let mut n = [0u8; 16];
            n[..s.len()].copy_from_slice(s.as_bytes());
            n
        };
        assert!(keeps_change_time(0x1000, &name("apfs")));
        assert!(keeps_change_time(0x1000 | 0x1, &name("hfs")));
        for kind in ["nfs", "smbfs", "msdos", "exfat", "macfuse", "apfsx", "hf"] {
            assert!(!keeps_change_time(0x1000, &name(kind)), "{kind}");
        }
        assert!(!keeps_change_time(0, &name("apfs")), "not local");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_volume_is_read_through_fstatfs() {
        // the temporary directory of this machine is on the boot volume
        let d = Dir::new();
        let p = d.0.join("a.css");
        std::fs::write(&p, "x").unwrap();
        let s = volume::of(&std::fs::File::open(&p).unwrap()).unwrap();
        assert_eq!(&s.type_name[..5], b"apfs\0");
        assert!(on_trusted_volume(&std::fs::File::open(&p).unwrap()));
    }

    #[test]
    fn a_write_moves_the_identity_even_under_the_old_time() {
        let d = Dir::new();
        let p = d.0.join("a.css");
        std::fs::write(&p, "p { color: red }").unwrap();
        let m = std::fs::metadata(&p).unwrap();
        let before = file_identity(&p, &m);
        std::fs::write(&p, "p { color: tan }").unwrap();
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(m.modified().unwrap()).unwrap();
        let after = std::fs::metadata(&p).unwrap();
        assert_eq!(
            (after.len(), after.modified().unwrap()),
            (m.len(), m.modified().unwrap())
        );
        assert_ne!(before, file_identity(&p, &after));
    }

    #[test]
    fn a_held_sheet_is_read_again_only_once_its_file_changed() {
        let d = Dir::new();
        let p = d.0.join("a.css");
        std::fs::write(&p, "p { color: red }").unwrap();
        let (dom, _) = crate::load::parse(
            b"<!doctype html><link rel=stylesheet href=a.css><p>x</p>",
            None,
        );
        let cache = SheetCache::new(1 << 20);
        let allow = [d.0.clone()];
        let open = || FileSheets::settling(Some(&d.0), Some(&allow), Duration::ZERO);
        let mut first = open();
        let red = style(&dom, &mut first, &cache);
        let mut again = open();
        let held = style(&dom, &mut again, &cache);
        assert_eq!((first.reads, again.reads), (1, 0));
        assert!(same(&dom, &red, &held));
        // as long as the old text, and modified at the old time
        let m = std::fs::metadata(&p).unwrap();
        std::fs::write(&p, "p { color: tan }").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(m.modified().unwrap())
            .unwrap();
        let mut changed = open();
        let tan = style(&dom, &mut changed, &cache);
        assert_eq!(changed.reads, 1);
        let want = style_document(&dom, Device::default(), &mut |_| {
            Some("p { color: tan }".into())
        });
        assert!(same(&dom, &want, &tan));
        assert!(!same(&dom, &red, &tan));
        let s = cache.stats();
        assert_eq!((s.parses, s.unread), (2, 1), "{s:?}");
    }

    #[test]
    fn an_unsettled_file_is_read_every_time() {
        let d = Dir::new();
        std::fs::write(d.0.join("a.css"), "p { color: red }").unwrap();
        let (dom, _) = crate::load::parse(
            b"<!doctype html><link rel=stylesheet href=a.css><p>x</p>",
            None,
        );
        let cache = SheetCache::new(1 << 20);
        let allow = [d.0.clone()];
        for _ in 0..2 {
            let mut links = FileSheets::new(Some(&d.0), Some(&allow));
            style(&dom, &mut links, &cache);
            assert_eq!(links.reads, 1);
        }
        let s = cache.stats();
        assert_eq!((s.parses, s.hits, s.unread), (1, 1, 0), "{s:?}");
    }

    #[test]
    fn a_file_outside_the_allowed_directories_is_not_named() {
        let d = Dir::new();
        let other = Dir::new();
        std::fs::write(d.0.join("a.css"), "p { color: red }").unwrap();
        let allow = [other.0.clone()];
        let mut links = FileSheets::settling(Some(&d.0), Some(&allow), Duration::ZERO);
        assert!(links.identity("a.css").is_none());
        assert!(links.fetch("a.css").is_none());
    }
}
