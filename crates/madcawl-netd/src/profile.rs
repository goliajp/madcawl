//! A profile: a directory (mode 0700) one network process owns at a
//! time, holding the cookies that outlive the process and the downloads.
//!
//! `lock` is held with `flock` for as long as the profile is open, so a
//! second process on the same directory is refused. `cookies.bin` is read
//! once at open; a change to a cookie that outlives the jar marks the
//! store dirty, and a writer thread saves it at most once per `COALESCE`,
//! by writing a temporary file and renaming it over the old one. With
//! nothing changed the thread sleeps on a condition variable, so an idle
//! profile never wakes. Closing the profile writes what is pending.

use crate::download::Downloads;
use madcawl_net::Client;
use madcawl_net::cookie::StoreError;
use madcawl_net::fetch::now_secs;
use std::fs::{DirBuilder, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const COOKIES: &str = "cookies.bin";
const COOKIES_TMP: &str = "cookies.bin.tmp";
/// where an unreadable store is moved; the previous one is replaced
const COOKIES_CORRUPT: &str = "cookies.bin.corrupt";
const COALESCE: Duration = Duration::from_secs(1);
const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;

unsafe extern "C" {
    fn flock(fd: i32, op: i32) -> i32;
}

#[derive(Debug)]
pub enum ProfileError {
    /// another process holds the profile
    InUse(PathBuf),
    Io(PathBuf, io::Error),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::InUse(p) => write!(f, "profile {} is in use by another network process", p.display()),
            ProfileError::Io(p, e) => write!(f, "profile {}: {e}", p.display()),
        }
    }
}

/// What opening found in the cookie store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loaded {
    /// no store yet
    Fresh,
    Cookies(usize),
    /// the store was unreadable and was moved aside to `cookies.bin.corrupt`
    Corrupt(StoreError),
}

#[derive(Default)]
struct State {
    dirty: bool,
    stop: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    /// one save at a time; the buffer is reused
    save: Mutex<Vec<u8>>,
    dir: PathBuf,
}

pub struct Profile {
    shared: Arc<Shared>,
    client: Arc<Client>,
    downloads: Arc<Downloads>,
    loaded: Loaded,
    writer: Option<JoinHandle<()>>,
    _lock: File,
}

fn private_dir(dir: &Path) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    std::fs::set_permissions(dir, Permissions::from_mode(0o700))
}

fn lock(dir: &Path) -> Result<File, ProfileError> {
    let path = dir.join("lock");
    let f = OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600).open(&path);
    let f = f.map_err(|e| ProfileError::Io(dir.to_owned(), e))?;
    if unsafe { flock(f.as_raw_fd(), LOCK_EX | LOCK_NB) } != 0 {
        let e = io::Error::last_os_error();
        return Err(match e.kind() {
            io::ErrorKind::WouldBlock => ProfileError::InUse(dir.to_owned()),
            _ => ProfileError::Io(dir.to_owned(), e),
        });
    }
    Ok(f)
}

/// Reads the store into the client's jar; an unreadable store is moved
/// aside and logged, and the jar stays empty.
fn load(dir: &Path, client: &Client) -> io::Result<Loaded> {
    let path = dir.join(COOKIES);
    let mut bytes = Vec::new();
    match File::open(&path) {
        Ok(f) => f.take(madcawl_net::cookie::MAX_STORE_FILE as u64 + 1).read_to_end(&mut bytes)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Loaded::Fresh),
        Err(e) => return Err(e),
    };
    match client.lock_jar().load_persistent(&bytes, now_secs()) {
        Ok(n) => Ok(Loaded::Cookies(n)),
        Err(e) => {
            std::fs::rename(&path, dir.join(COOKIES_CORRUPT))?;
            eprintln!("madcawl-netd: {}: {e}; moved aside to {COOKIES_CORRUPT}, starting with no cookies", path.display());
            Ok(Loaded::Corrupt(e))
        }
    }
}

impl Shared {
    /// Writes the jar's persistent cookies to the store.
    fn save(&self, client: &Client) -> io::Result<()> {
        let mut buf = self.save.lock().unwrap_or_else(PoisonError::into_inner);
        client.lock_jar().encode_persistent(now_secs(), &mut buf);
        let tmp = self.dir.join(COOKIES_TMP);
        let mut f = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(&buf)?;
        f.sync_all()?;
        std::fs::rename(&tmp, self.dir.join(COOKIES))?;
        File::open(&self.dir)?.sync_all()
    }

    fn run(&self, client: &Client) {
        loop {
            let mut st = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            while !st.dirty && !st.stop {
                st = self.wake.wait(st).unwrap_or_else(PoisonError::into_inner);
            }
            let due = Instant::now() + COALESCE;
            while !st.stop && let Some(left) = due.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) {
                st = self.wake.wait_timeout(st, left).unwrap_or_else(PoisonError::into_inner).0;
            }
            if st.stop {
                return;
            }
            st.dirty = false;
            drop(st);
            if let Err(e) = self.save(client) {
                eprintln!("madcawl-netd: {}: saving cookies: {e}", self.dir.display());
            }
        }
    }
}

impl Profile {
    /// Opens the profile at `dir`, creating it, and loads its cookies
    /// into `client`'s jar.
    pub fn open(dir: &Path, client: Arc<Client>) -> Result<Profile, ProfileError> {
        let err = |e| ProfileError::Io(dir.to_owned(), e);
        private_dir(dir).map_err(err)?;
        let lock = lock(dir)?;
        let loaded = load(dir, &client).map_err(err)?;
        let downloads = Downloads::new(dir.join("downloads"));
        private_dir(&downloads.dir).map_err(err)?;
        downloads.remove_partial();
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            save: Mutex::new(Vec::new()),
            dir: dir.to_owned(),
        });
        let hook = shared.clone();
        client.lock_jar().set_on_change(move || {
            hook.state.lock().unwrap_or_else(PoisonError::into_inner).dirty = true;
            hook.wake.notify_one();
        });
        let (s, c) = (shared.clone(), client.clone());
        let writer = std::thread::Builder::new().name("netd-profile".into()).spawn(move || s.run(&c)).map_err(err)?;
        Ok(Profile { shared, client, downloads: Arc::new(downloads), loaded, writer: Some(writer), _lock: lock })
    }

    pub fn loaded(&self) -> Loaded {
        self.loaded
    }

    pub fn downloads(&self) -> Arc<Downloads> {
        self.downloads.clone()
    }

    /// Writes the cookies now if anything changed since the last save.
    pub fn flush(&self) -> io::Result<()> {
        let dirty = std::mem::take(&mut self.shared.state.lock().unwrap_or_else(PoisonError::into_inner).dirty);
        if dirty { self.shared.save(&self.client) } else { Ok(()) }
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap_or_else(PoisonError::into_inner).stop = true;
        self.shared.wake.notify_one();
        if let Some(t) = self.writer.take() {
            let _ = t.join();
        }
        if let Err(e) = self.flush() {
            eprintln!("madcawl-netd: {}: saving cookies: {e}", self.shared.dir.display());
        }
    }
}
