//! Keeping the jar within its caps.

use super::CookieJar;

impl CookieJar {
    /// Drops expired cookies, then the least recently used one of
    /// `domain` when it is at its cap, then the least recently used ones
    /// of the jar until it is under its total.
    pub(super) fn make_room(&mut self, domain: &str, now: i64) {
        for list in self.by_domain.values_mut() {
            let before = list.len();
            list.retain(|c| c.expires.is_none_or(|e| e >= now));
            self.total -= before - list.len();
        }
        if let Some(list) = self.by_domain.get_mut(domain)
            && list.len() >= self.limits.per_domain
            && let Some(i) = list.iter().enumerate().min_by_key(|(_, c)| c.accessed).map(|(i, _)| i)
        {
            list.remove(i);
            self.total -= 1;
        }
        while self.total >= self.limits.total {
            let oldest = self
                .by_domain
                .iter()
                .flat_map(|(d, l)| l.iter().enumerate().map(move |(i, c)| (c.accessed, d.clone(), i)))
                .min();
            let Some((_, d, i)) = oldest else { break };
            self.by_domain.get_mut(&d).expect("domain just seen").remove(i);
            self.total -= 1;
        }
        self.by_domain.retain(|_, l| !l.is_empty());
    }
}
