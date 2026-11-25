//! A session's request rate: a token bucket, and the audit log's view
//! of what it refused, one line a second however many were refused.

use std::time::{Duration, Instant};

pub(crate) struct Bucket {
    tokens: f64,
    rate: f64,
    burst: f64,
    last: Instant,
    /// Refused since the last line about them, and when that was
    refused: u64,
    logged: Option<Instant>,
}

impl Bucket {
    pub fn new(rate: f64, burst: f64) -> Bucket {
        Bucket {
            tokens: burst,
            rate,
            burst,
            last: Instant::now(),
            refused: 0,
            logged: None,
        }
    }

    /// Whether a request may go ahead now.
    pub fn take(&mut self, now: Instant) -> bool {
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + dt * self.rate).min(self.burst);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            return true;
        }
        self.refused += 1;
        false
    }

    /// How many were refused since the last audit line about them, when
    /// a line is due (a second after the last), or at the end (`all`).
    pub fn due(&mut self, now: Instant, all: bool) -> Option<u64> {
        let quiet = self
            .logged
            .is_none_or(|t| now.saturating_duration_since(t) >= Duration::from_secs(1));
        if self.refused == 0 || !(quiet || all) {
            return None;
        }
        self.logged = Some(now);
        Some(std::mem::take(&mut self.refused))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_then_the_rate_and_one_line_a_second() {
        let t = Instant::now();
        let mut b = Bucket::new(10.0, 3.0);
        assert!((0..3).all(|_| b.take(t)));
        assert!(!b.take(t) && !b.take(t));
        assert_eq!(b.due(t, false), Some(2));
        assert!(!b.take(t));
        assert_eq!(b.due(t, false), None, "a line a second");
        assert_eq!(b.due(t, true), Some(1));
        assert!(b.take(t + Duration::from_millis(150)));
    }
}
