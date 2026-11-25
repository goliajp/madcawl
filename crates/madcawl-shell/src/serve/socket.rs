//! Binding the host's socket so only the user can reach it: never in a
//! directory anyone may swap files in, never over a file that is not a
//! socket, and created with no access for others from the start.

use std::io::{Error, ErrorKind};
use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
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
    match std::fs::symlink_metadata(path) {
        Ok(m) if !m.file_type().is_socket() => {
            return Err(Error::new(
                ErrorKind::AlreadyExists,
                "something that is not a socket is at the socket path",
            ));
        }
        Ok(_) if UnixStream::connect(path).is_ok() => {
            return Err(Error::new(
                ErrorKind::AddrInUse,
                "another host is serving on this socket",
            ));
        }
        Ok(_) => std::fs::remove_file(path)?,
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let guard = UMASK.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: umask only swaps the process's file mode mask
    let old = unsafe { umask(0o077) };
    let bound = UnixListener::bind(path);
    // SAFETY: as above, putting the old mask back
    unsafe { umask(old) };
    drop(guard);
    bound
}
