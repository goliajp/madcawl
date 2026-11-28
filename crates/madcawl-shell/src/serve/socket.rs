//! Binding the host's socket so only the user can reach it: never in a
//! directory anyone may swap files in, never over a file that is not a
//! socket, and created with no access for others from the start.

use std::io::{Error, ErrorKind};
use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[cfg(target_os = "macos")]
type ModeT = u16;
#[cfg(not(target_os = "macos"))]
type ModeT = u32;

unsafe extern "C" {
    fn umask(mask: ModeT) -> ModeT;
}

/// The umask is the process's: two binds at once must not restore each
/// other's value.
static UMASK: Mutex<()> = Mutex::new(());

/// A listener on `path`, which appears only once it listens: a client
/// that finds the path never meets a socket that refuses it.
pub fn bind(path: &Path) -> std::io::Result<UnixListener> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let mode = std::fs::metadata(parent)?.permissions().mode();
    if mode & 0o002 != 0 && mode & 0o1000 == 0 {
        return Err(Error::other(format!(
            "{} is writable by anyone and not sticky: choose another place for the socket",
            parent.display()
        )));
    }
    clear(path)?;
    // bind makes the file and listen comes after it, so the listener
    // binds a name of this process's and is linked into place listening
    let mut staged = path.as_os_str().to_owned();
    staged.push(format!(".{}", std::process::id()));
    let staged = PathBuf::from(staged);
    clear(&staged)?;
    let listener = bind_private(&staged)?;
    let linked = std::fs::hard_link(&staged, path);
    std::fs::remove_file(&staged)?;
    match linked {
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Err(in_use()),
        r => r.map(|()| listener),
    }
}

/// Removes a socket at `path` that no host serves on.
fn clear(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if !m.file_type().is_socket() => Err(Error::new(
            ErrorKind::AlreadyExists,
            "something that is not a socket is at the socket path",
        )),
        Ok(_) if UnixStream::connect(path).is_ok() => Err(in_use()),
        // another host starting at the same time may have removed it first
        Ok(_) => match std::fs::remove_file(path) {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            r => r,
        },
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn in_use() -> Error {
    Error::new(ErrorKind::AddrInUse, "another host is serving on this socket")
}

/// Binds with no access for others from the start.
fn bind_private(path: &Path) -> std::io::Result<UnixListener> {
    let guard = UMASK.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: umask only swaps the process's file mode mask
    let old = unsafe { umask(0o077) };
    let bound = UnixListener::bind(path);
    // SAFETY: as above, putting the old mask back
    unsafe { umask(old) };
    drop(guard);
    bound
}

#[cfg(test)]
#[path = "socket_tests.rs"]
mod tests;
