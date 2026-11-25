//! A page's log of requests: a bounded ring of what each
//! request was — method, URL, status, size, how long to the response
//! head and to the end — and a bounded ring of load errors. No bodies,
//! no headers: the URLs come in with the values of password fields
//! already masked by the page.

use std::collections::VecDeque;
use std::fmt::Write as _;

/// Requests a page's log holds; older ones are dropped and counted.
pub const LOG_CAP: usize = 256;
/// Load errors a page keeps.
pub const ERROR_CAP: usize = 64;
/// The longest URL written to the log.
const MAX_URL: usize = 2048;

#[derive(Clone, Debug)]
pub struct Entry {
    pub what: &'static str,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub bytes: u64,
    pub head_ms: Option<f64>,
    pub total_ms: f64,
    /// Why it failed, when it did
    pub error: Option<String>,
}

#[derive(Default)]
pub struct RequestLog {
    entries: VecDeque<Entry>,
    errors: VecDeque<String>,
    dropped: u64,
    total: u64,
}

fn cut(s: &str) -> String {
    let mut out: String = s.chars().filter(|c| !c.is_control()).take(MAX_URL).collect();
    if s.chars().count() > MAX_URL {
        out.push('…');
    }
    out
}

impl RequestLog {
    pub fn push(&mut self, mut e: Entry) {
        e.url = cut(&e.url);
        if let Some(why) = &e.error {
            let kind = if why.starts_with("certificate") || why.starts_with("tls") {
                "certificate error"
            } else {
                "load failed"
            };
            self.error(format!("{kind}: {} {}: {why}", e.method, e.url));
        } else if e.what == "document" && e.status.is_some_and(|s| s >= 400) {
            self.error(format!("http {}: {} {}", e.status.unwrap_or(0), e.method, e.url));
        }
        if self.entries.len() == LOG_CAP {
            self.entries.pop_front();
            self.dropped += 1;
        }
        self.total += 1;
        self.entries.push_back(e);
    }

    pub fn error(&mut self, line: String) {
        if self.errors.len() == ERROR_CAP {
            self.errors.pop_front();
        }
        self.errors.push_back(cut(&line));
    }

    /// The last `limit` requests, oldest first, then the errors.
    pub fn text(&self, limit: usize, in_flight: &[String]) -> String {
        let mut out = format!("{} requests ({} dropped from the log)\n", self.total, self.dropped);
        let skip = self.entries.len().saturating_sub(limit.max(1));
        for e in self.entries.iter().skip(skip) {
            let status = e.status.map_or("-".into(), |s| s.to_string());
            let head = e.head_ms.map_or("-".into(), |h| format!("{h:.1}"));
            let _ = write!(
                out,
                "{} {} {} {status} {}B head {head}ms total {:.1}ms",
                e.what, e.method, e.url, e.bytes, e.total_ms
            );
            if let Some(why) = &e.error {
                let _ = write!(out, " error: {why}");
            }
            out.push('\n');
        }
        for f in in_flight {
            let _ = writeln!(out, "in flight: {f}");
        }
        if self.errors.is_empty() {
            out.push_str("no load errors\n");
        }
        for e in &self.errors {
            let _ = writeln!(out, "error: {e}");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(n: usize) -> Entry {
        Entry {
            what: "document",
            method: "GET".into(),
            url: format!("http://127.0.0.1/{n}"),
            status: Some(if n == 3 { 404 } else { 200 }),
            bytes: 10,
            head_ms: Some(1.0),
            total_ms: 2.0,
            error: None,
        }
    }

    #[test]
    fn the_log_is_a_bounded_ring_and_errors_are_kept_apart() {
        let mut l = RequestLog::default();
        for n in 0..LOG_CAP + 5 {
            l.push(entry(n));
        }
        assert_eq!(l.entries.len(), LOG_CAP);
        let t = l.text(2, &[]);
        assert!(t.starts_with(&format!("{} requests (5 dropped", LOG_CAP + 5)), "{t}");
        assert!(t.contains(&format!("/{} 200", LOG_CAP + 4)) && !t.contains(&format!("/{} ", LOG_CAP + 2)));
        assert!(t.contains("error: http 404: GET http://127.0.0.1/3"), "{t}");
        let mut e = entry(9);
        e.error = Some("certificate not trusted: expired".into());
        e.url = format!("https://x/{}", "a\n".repeat(3000));
        l.push(e);
        let t = l.text(1, &["GET http://127.0.0.1/slow".into()]);
        assert!(t.contains("error: certificate error: GET https://x/aaa"), "{t}");
        assert!(t.contains("in flight: GET http://127.0.0.1/slow"));
        assert!(t.lines().all(|line| line.len() < 4 * MAX_URL));
    }
}
