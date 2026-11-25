//! Keep-alive connections by origin. At most `per_origin` connections to
//! one origin are in use at a time (more requests wait), at most
//! `max_idle` sit idle in all (the longest idle goes first), and an idle
//! connection older than `idle_timeout` is closed when next looked at.
//! There is no timer: expiry is checked on checkout and checkin.

use crate::error::NetError;
use crate::stream::Stream;
use std::collections::HashMap;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub tls: bool,
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct PoolLimits {
    pub per_origin: usize,
    pub max_idle: usize,
    pub idle_timeout: Duration,
}

impl Default for PoolLimits {
    fn default() -> Self {
        PoolLimits { per_origin: 6, max_idle: 32, idle_timeout: Duration::from_secs(60) }
    }
}

#[derive(Default)]
struct State {
    idle: HashMap<Key, Vec<(Stream, Instant)>>,
    idle_count: usize,
    active: HashMap<Key, usize>,
}

pub struct Pool {
    limits: PoolLimits,
    state: Mutex<State>,
    freed: Condvar,
}

/// A claim on one of an origin's connection slots, given back on drop.
pub struct Slot<'a> {
    pool: &'a Pool,
    key: Key,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        let mut s = self.pool.state.lock().unwrap();
        if let Some(n) = s.active.get_mut(&self.key) {
            *n -= 1;
            if *n == 0 {
                s.active.remove(&self.key);
            }
        }
        drop(s);
        self.pool.freed.notify_all();
    }
}

impl Slot<'_> {
    pub fn key(&self) -> &Key {
        &self.key
    }
}

impl Pool {
    pub fn new(limits: PoolLimits) -> Self {
        Pool { limits, state: Mutex::new(State::default()), freed: Condvar::new() }
    }

    pub fn idle_count(&self) -> usize {
        self.state.lock().unwrap().idle_count
    }

    /// Takes a slot for `key`, waiting up to `wait` for one, and the most
    /// recently idled live connection to it, if any.
    pub fn checkout(&self, key: &Key, wait: Duration) -> Result<(Slot<'_>, Option<Stream>), NetError> {
        let deadline = Instant::now() + wait;
        let mut s = self.state.lock().unwrap();
        while s.active.get(key).copied().unwrap_or(0) >= self.limits.per_origin {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(NetError::ConnectTimeout);
            }
            s = self.freed.wait_timeout(s, left).unwrap().0;
        }
        *s.active.entry(key.clone()).or_default() += 1;
        let slot = Slot { pool: self, key: key.clone() };
        let mut found = None;
        let mut closed = Vec::new();
        if let Some(list) = s.idle.get_mut(key) {
            while let Some((stream, since)) = list.pop() {
                if since.elapsed() < self.limits.idle_timeout && !stream.is_stale() {
                    found = Some(stream);
                    break;
                }
                closed.push(stream);
            }
            if list.is_empty() {
                s.idle.remove(key);
            }
        }
        s.idle_count -= closed.len() + usize::from(found.is_some());
        drop(s);
        drop(closed);
        Ok((slot, found))
    }

    /// Puts a connection that finished an exchange cleanly back as idle.
    pub fn checkin(&self, slot: Slot<'_>, stream: Stream) {
        let mut evicted = Vec::new();
        let mut s = self.state.lock().unwrap();
        let timeout = self.limits.idle_timeout;
        for list in s.idle.values_mut() {
            let (keep, old): (Vec<_>, Vec<_>) = list.drain(..).partition(|(_, t)| t.elapsed() < timeout);
            *list = keep;
            evicted.extend(old.into_iter().map(|(st, _)| st));
        }
        s.idle_count -= evicted.len();
        s.idle.retain(|_, l| !l.is_empty());
        if self.limits.max_idle > 0 {
            while s.idle_count >= self.limits.max_idle {
                let oldest = s
                    .idle
                    .iter()
                    .filter_map(|(k, l)| l.first().map(|(_, t)| (k.clone(), *t)))
                    .min_by_key(|(_, t)| *t)
                    .map(|(k, _)| k);
                let Some(k) = oldest else { break };
                let list = s.idle.get_mut(&k).expect("key just seen");
                evicted.push(list.remove(0).0);
                if list.is_empty() {
                    s.idle.remove(&k);
                }
                s.idle_count -= 1;
            }
            s.idle.entry(slot.key.clone()).or_default().push((stream, Instant::now()));
            s.idle_count += 1;
        } else {
            evicted.push(stream);
        }
        drop(s);
        drop(evicted);
        drop(slot);
    }
}
