//! A session's screenshot requests: the page checked to be the
//! session's own, the picture taken, and the reply — what was drawn and
//! the marked elements, in an envelope, with the PNG beside it or saved
//! as a new file in the session's downloads directory.

use std::fs::{DirBuilder, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use madcawl_agent::{Frame, kind, tag};

use super::Conn;
use crate::serve::shot::{Area, MAX_SHOT_BYTES, ShotRequest, Taken, describe};
use crate::serve::{code, error_frame};

/// Entries the downloads directory may hold before it counts as full.
const MAX_ENTRIES: u64 = 10_000;

impl Conn<'_> {
    pub(super) fn screenshot(&mut self, f: &Frame) -> Frame {
        let req = match ShotRequest::from_frame(f) {
            Ok(r) if r.file.is_some() && self.shared.config.downloads.is_none() => Err((
                code::NOT_ALLOWED,
                "saving a screenshot needs a downloads directory, which the host was not started with".to_owned(),
            )),
            Ok(r) => Ok(r),
            Err(m) => Err((code::BAD_REQUEST, m)),
        };
        let req = match req {
            Ok(r) => r,
            Err((c, m)) => {
                let r = error_frame(c, &m);
                self.log("-", "screenshot", ("", ""), &r);
                return r;
            }
        };
        let i = match self.page_for(f, "screenshot") {
            Ok(i) => i,
            Err(r) => return r,
        };
        let p = &mut self.pages[i];
        let target = match &req.area {
            Area::Element(r) => p.target(r),
            _ => None,
        };
        let url = p.url.clone();
        crate::serve::watch::stage("paint");
        let taken = p.screenshot(&req);
        let seq = p.sync();
        let (reply, saved) = match taken {
            Err(m) => (error_frame(code::CHECK, &m), None),
            Ok(t) => self.shot_reply(&req, &t, seq, &url),
        };
        let action = match &saved {
            Some(name) => format!("screenshot to {name}"),
            None => "screenshot".to_owned(),
        };
        let (role, name) = target.unwrap_or(("", String::new()));
        self.log(&url, &action, (role, &name), &reply);
        reply
    }

    /// The reply to a picture taken, and the name of the file it was
    /// saved as.
    fn shot_reply(
        &self,
        req: &ShotRequest,
        t: &Taken,
        seq: u64,
        url: &str,
    ) -> (Frame, Option<String>) {
        let mut text = describe(req, t);
        let mut saved = None;
        if let Some(wanted) = &req.file {
            let config = &self.shared.config;
            let root = config
                .downloads
                .as_deref()
                .expect("checked before the picture was taken");
            let dir = root.join(format!("s{}-{}", self.id, self.shared.run));
            match save(root, &dir, wanted, &t.png, config.download_cap) {
                Ok(path) => {
                    text.push_str(&format!("saved to {}\n", path.display()));
                    saved = path.file_name().map(|n| n.to_string_lossy().into_owned());
                }
                Err((c, m)) => return (error_frame(c, &m), None),
            }
        } else if t.png.len() > MAX_SHOT_BYTES {
            let m = format!(
                "the picture would be {} bytes, more than the {MAX_SHOT_BYTES} one may be; pass a smaller scale or area",
                t.png.len()
            );
            return (error_frame(code::REPLY_TOO_LARGE, &m), None);
        }
        let r = Frame::new(kind::RESULT)
            .num(tag::SEQ, seq)
            .num(tag::WIDTH, t.width as u64)
            .num(tag::HEIGHT, t.height as u64)
            .str(tag::TEXT, &text);
        let reply = match self.wrap(r, url) {
            w if w.kind == kind::ERROR => return (w, None),
            w if saved.is_some() => w,
            w => w.bytes(tag::DATA, &t.png),
        };
        (reply, saved)
    }
}

/// Bytes the files below `root` hold, links not followed; None past
/// [`MAX_ENTRIES`] entries.
fn used(root: &Path) -> Option<u64> {
    let (mut bytes, mut entries) = (0u64, 0u64);
    let mut dirs = vec![root.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            entries += 1;
            if entries > MAX_ENTRIES {
                return None;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => dirs.push(e.path()),
                Ok(t) if t.is_file() => {
                    bytes = bytes.saturating_add(e.metadata().map_or(0, |m| m.len()))
                }
                _ => {}
            }
        }
    }
    Some(bytes)
}

/// Writes `png` as a new file in `dir`, the session's directory below the
/// downloads directory `root`, named after `wanted` made safe; a file is
/// at most `cap` bytes and the whole directory four times that, as for
/// downloads. Where it went, or the error code and why not.
pub(crate) fn save(
    root: &Path,
    dir: &Path,
    wanted: &str,
    png: &[u8],
    cap: u64,
) -> Result<PathBuf, (u64, String)> {
    let mut name = madcawl_netd::sanitize(wanted);
    if !name.to_ascii_lowercase().ends_with(".png") {
        name.push_str(".png");
    }
    let size = png.len() as u64;
    if size > cap {
        return Err((
            code::NOT_ALLOWED,
            format!("the picture is {size} bytes, more than the {cap} a download may be"),
        ));
    }
    if used(root).is_none_or(|u| u.saturating_add(size) > cap.saturating_mul(4)) {
        return Err((code::NOT_ALLOWED, "the downloads directory is full".into()));
    }
    let io = |e: std::io::Error| (code::IO, format!("saving the picture failed: {e}"));
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(io)?;
    if std::fs::symlink_metadata(dir)
        .map_err(io)?
        .file_type()
        .is_symlink()
    {
        return Err((
            code::IO,
            "the session's downloads directory is a link".into(),
        ));
    }
    let path = dir.join(&name);
    let mut f = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            return Err((
                code::IO,
                format!("a file named {name} is already there; pass another name"),
            ));
        }
        Err(e) => return Err(io(e)),
    };
    if let Err(e) = f.write_all(png) {
        let _ = std::fs::remove_file(&path);
        return Err(io(e));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_saved_as_a_new_safe_file_within_the_caps() {
        let root = std::env::temp_dir().join(format!("madcawl-shot-save-{}", std::process::id()));
        let dir = root.join("s1-t");
        let p = save(&root, &dir, "../../etc/x", b"png", 100).unwrap();
        assert_eq!(p, dir.join("x.png"), "no path, a .png name");
        assert_eq!(std::fs::read(&p).unwrap(), b"png");
        let again = save(&root, &dir, "x.png", b"png", 100).unwrap_err();
        assert!(again.1.contains("already there"), "{again:?}");
        assert_eq!(
            std::fs::read(&p).unwrap(),
            b"png",
            "an existing file is never truncated"
        );
        std::os::unix::fs::symlink(root.join("elsewhere"), dir.join("link.png")).unwrap();
        assert!(
            save(&root, &dir, "link.png", b"png", 100).is_err(),
            "a link is not followed"
        );
        assert!(!root.join("elsewhere").exists());
        assert!(
            save(&root, &dir, "big", &[0; 101], 100)
                .unwrap_err()
                .1
                .contains("more than")
        );
        std::fs::write(dir.join("filler"), [0u8; 395]).unwrap();
        assert!(
            save(&root, &dir, "full", &[0; 10], 100)
                .unwrap_err()
                .1
                .contains("full")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
