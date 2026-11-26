//! Audit lines from the content process, written under the session's
//! number whatever they say, and no more than `LINES` of them in each
//! `WINDOW`: past that they are dropped and counted, and one line says
//! how many went once the next window starts or the session ends. Each
//! field is cut short when written, so the cap bounds the bytes too.

use std::time::{Duration, Instant};

use madcawl_agent::{Frame, tag};

use super::Proxy;
use crate::serve::log::Line;

const WINDOW: Duration = Duration::from_secs(10);
const LINES: u32 = 2000;

/// What the session's content process has written in the current window.
pub(super) struct Budget {
    pub since: Instant,
    written: u32,
    dropped: u64,
}

impl Budget {
    pub fn new() -> Budget {
        Budget { since: Instant::now(), written: 0, dropped: 0 }
    }
}

impl Proxy<'_> {
    /// Takes one line from the window's budget; false when it is spent and
    /// the line is to be dropped (and counted).
    fn charge(&mut self) -> bool {
        if self.audit.since.elapsed() >= WINDOW {
            self.audit_dropped();
            self.audit.since = Instant::now();
            self.audit.written = 0;
        }
        if self.audit.written >= LINES {
            self.audit.dropped += 1;
            return false;
        }
        self.audit.written += 1;
        true
    }

    /// A line the host writes because of a content-process frame: it is
    /// held to the same budget as the process's own lines.
    pub(super) fn content_log(&mut self, action: &str, result: &str) {
        if self.charge() {
            self.log(action, result);
        }
    }

    pub(super) fn audit(&mut self, q: &Frame) {
        if !self.charge() {
            return;
        }
        let f: Vec<&str> = q.fields.iter().filter(|(t, _)| *t == tag::FIELD).map(|(_, v)| std::str::from_utf8(v).unwrap_or("?")).collect();
        let [origin, action, role, name, result] = f[..] else {
            self.log("audit", "a malformed line from the content process");
            return;
        };
        let line = Line { session: self.id, origin, action, role, name, result };
        self.shared.log.write(&line);
    }

    /// Writes how many lines were dropped since this was last written.
    pub(super) fn audit_dropped(&mut self) {
        let n = std::mem::take(&mut self.audit.dropped);
        if n > 0 {
            self.log("audit-dropped", &format!("{n} lines from the content process over {LINES} in {} s", WINDOW.as_secs()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve::link::internal;
    use crate::serve::proxy::tests::shared;

    #[test]
    fn a_content_process_flooding_audit_lines_is_held_to_its_budget() {
        let (s, dir) = shared("audit", false, |_| {});
        let mut p = Proxy::new(1, &s);
        let line = ["file://", "read", "", "", "ok"].iter().fold(Frame::new(internal::AUDIT), |f, v| f.str(tag::FIELD, v));
        for _ in 0..LINES + 500 {
            p.audit(&line);
        }
        let log = || std::fs::read_to_string(dir.0.join("audit.log")).unwrap();
        assert_eq!(log().lines().count(), LINES as usize);
        p.audit.since -= WINDOW;
        p.audit(&line);
        let text = log();
        let tail: Vec<&str> = text.lines().skip(LINES as usize).collect();
        assert_eq!(tail.len(), 2, "{tail:?}");
        assert!(tail[0].contains("audit-dropped") && tail[0].contains("500 lines"), "{}", tail[0]);
        assert!(tail[1].contains("\tread\t"), "{}", tail[1]);
    }

    #[test]
    fn refused_questions_from_a_content_process_are_held_to_the_same_budget() {
        let (s, dir) = shared("audit-q", false, |_| {});
        let mut p = Proxy::new(1, &s);
        let unknown = Frame::new(200);
        for _ in 0..LINES + 500 {
            assert!(p.answer(&unknown).is_some());
        }
        let log = std::fs::read_to_string(dir.0.join("audit.log")).unwrap();
        assert_eq!(log.lines().count(), LINES as usize);
        assert_eq!(p.audit.dropped, 500);
    }
}
