//! Cookie partitions: jars beside the client's own, one for each agent
//! session its host names, so that no partition sees the cookies of
//! another or of the client's own jar, which holds the user's logins and
//! is the only one a profile saves. A partition lives in memory only,
//! from when the host opens it until the host drops it; at most `max`
//! are open at a time, each jar under its own limits.

use crate::cookie::{CookieJar, JarLimits};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub type PartitionId = u64;
/// the client's own jar
pub const PROFILE: PartitionId = 0;

pub type SharedJar = Arc<Mutex<CookieJar>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionError {
    /// `PROFILE` names the client's own jar, which is never opened or dropped
    Reserved,
    /// as many partitions are open as the limit allows
    Full(usize),
}

impl std::fmt::Display for PartitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartitionError::Reserved => f.write_str("partition 0 is the profile's"),
            PartitionError::Full(n) => write!(f, "too many cookie partitions: at most {n}"),
        }
    }
}

pub struct Partitions {
    map: Mutex<HashMap<PartitionId, SharedJar>>,
    max: usize,
    limits: JarLimits,
}

/// A jar's lock; one poisoned by a panic elsewhere is taken over, since
/// every change to a jar leaves it consistent.
pub fn lock(jar: &Mutex<CookieJar>) -> MutexGuard<'_, CookieJar> {
    jar.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Partitions {
    pub fn new(max: usize, limits: JarLimits) -> Self {
        Partitions { map: Mutex::new(HashMap::new()), max, limits }
    }

    fn map(&self) -> MutexGuard<'_, HashMap<PartitionId, SharedJar>> {
        self.map.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Opens partition `id` with an empty jar; true when it was made now,
    /// false when it was open already.
    pub fn open(&self, id: PartitionId) -> Result<bool, PartitionError> {
        if id == PROFILE {
            return Err(PartitionError::Reserved);
        }
        let mut map = self.map();
        if map.contains_key(&id) {
            return Ok(false);
        }
        if map.len() >= self.max {
            return Err(PartitionError::Full(self.max));
        }
        map.insert(id, Arc::new(Mutex::new(CookieJar::new(self.limits))));
        Ok(true)
    }

    pub fn get(&self, id: PartitionId) -> Option<SharedJar> {
        self.map().get(&id).cloned()
    }

    pub fn contains(&self, id: PartitionId) -> bool {
        self.map().contains_key(&id)
    }

    /// Drops partition `id` and its cookies; false when it was not open.
    /// A fetch already running in it finishes on the dropped jar.
    pub fn close(&self, id: PartitionId) -> bool {
        self.map().remove(&id).is_some()
    }

    pub fn len(&self) -> usize {
        self.map().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn max(&self) -> usize {
        self.max
    }
}
