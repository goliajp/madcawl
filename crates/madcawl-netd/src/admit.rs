//! Admission: how many clients, requests and request-body bytes the
//! service takes at a time, and the slots a running request holds.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Requests are shared out so that no client can lock the others out:
/// each client may always run `reserve()` requests, and runs more only by
/// drawing on a shared pool of what is left of `requests` once every
/// possible client's reserve is set aside. A client that holds up to
/// `in_flight` requests leaves the others their reserves; a cap of
/// `clients × in_flight` in all would not be needed for that, and would
/// allow that many threads.
#[derive(Clone, Copy, Debug)]
pub struct ServerLimits {
    pub clients: usize,
    /// requests at a time from one client
    pub in_flight: usize,
    /// requests at a time from all clients together
    pub requests: usize,
    /// bytes of request bodies held at a time by all clients together
    pub body_bytes: usize,
    /// how long a write to a client may block before the client is dropped
    pub write_timeout: Duration,
    /// how long a client served with a control channel may take, in all,
    /// to bind itself with HELLO before it is dropped
    pub hello_timeout: Duration,
}

impl Default for ServerLimits {
    fn default() -> Self {
        ServerLimits {
            clients: 64,
            in_flight: 32,
            requests: 256,
            body_bytes: 64 << 20,
            write_timeout: Duration::from_secs(30),
            hello_timeout: Duration::from_secs(10),
        }
    }
}

impl ServerLimits {
    /// Requests each client may run without drawing on the shared pool;
    /// all reserves together are at most half of `requests`.
    pub fn reserve(&self) -> usize {
        self.requests / (2 * self.clients.max(1))
    }

    fn shared(&self) -> usize {
        self.requests - self.clients * self.reserve()
    }
}

/// `n` units under a cap, given back when dropped.
pub(crate) struct Slot(Arc<AtomicUsize>, usize);

impl Slot {
    pub(crate) fn take(count: &Arc<AtomicUsize>, n: usize, cap: usize) -> Option<Slot> {
        if count.fetch_add(n, Ordering::SeqCst).saturating_add(n) > cap {
            count.fetch_sub(n, Ordering::SeqCst);
            return None;
        }
        Some(Slot(count.clone(), n))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(self.1, Ordering::SeqCst);
    }
}

/// The counters all clients share.
#[derive(Clone, Default)]
pub(crate) struct Shared {
    /// requests drawn from the shared pool
    requests: Arc<AtomicUsize>,
    body_bytes: Arc<AtomicUsize>,
}

/// What one running request holds: its place among its client's, a place
/// in the shared pool unless it is within the client's reserve, and its
/// body's bytes.
pub(crate) struct Held {
    _mine: Slot,
    _shared: Option<Slot>,
    _body: Slot,
}

pub(crate) fn admit(mine: &Arc<AtomicUsize>, shared: &Shared, limits: &ServerLimits, body: usize) -> Result<Held, &'static str> {
    const BUSY: &str = "too many requests in flight";
    // only this client's reader adds to `mine`, so it cannot grow meanwhile
    let within_reserve = mine.load(Ordering::SeqCst) < limits.reserve();
    let _mine = Slot::take(mine, 1, limits.in_flight).ok_or(BUSY)?;
    let _shared = match within_reserve {
        true => None,
        false => Some(Slot::take(&shared.requests, 1, limits.shared()).ok_or(BUSY)?),
    };
    let _body = Slot::take(&shared.body_bytes, body, limits.body_bytes).ok_or("request bodies over their memory budget")?;
    Ok(Held { _mine, _shared, _body })
}
