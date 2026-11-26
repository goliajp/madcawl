//! The session's page list and the host's counters.

use std::sync::atomic::Ordering;
use std::time::Instant;

use madcawl_agent::{Frame, kind, tag};

use super::Conn;

impl Conn<'_> {
    pub(super) fn list(&self) -> Frame {
        let now = Instant::now();
        let mut text = String::new();
        for p in &self.pages {
            text.push_str(&format!(
                "p{} {} {}x{} idle {}s\n",
                p.id,
                p.url,
                p.width,
                p.height,
                now.duration_since(p.last_used).as_secs()
            ));
        }
        if text.is_empty() {
            text.push_str("no pages open\n");
        }
        let reply = Frame::new(kind::RESULT).str(tag::TEXT, &text);
        self.log("-", "pages", ("", ""), &reply);
        reply
    }

    /// the host's counters, a `name value` pair a line
    pub(super) fn stats(&self) -> Frame {
        let (s, cap) = crate::serve::doc::sheet_stats();
        let text = format!(
            "sheet-cache-weight {}\nsheet-cache-cap {}\nsheet-cache-entries {}\nsheet-cache-names {}\n\
             sheet-cache-parses {}\nsheet-cache-hits {}\nsheet-cache-unread {}\npages {}\n",
            s.weight,
            cap,
            s.entries,
            s.names,
            s.parses,
            s.hits,
            s.unread,
            self.shared.pages.load(Ordering::SeqCst),
        );
        let mut text = text;
        if let Some(p) = self.shared.net.as_ref().and_then(|n| n.process.as_ref()) {
            text.push_str(&format!("netd-pid {}\nnetd-restarts {}\n", p.pid(), p.restarts.load(Ordering::SeqCst)));
        }
        let reply = Frame::new(kind::RESULT).str(tag::TEXT, &text);
        self.log("-", "stats", ("", ""), &reply);
        reply
    }
}
