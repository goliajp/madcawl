//! A content process that ended, recovered: another one started and
//! the session's pages opened in it again, unless the crash brake holds.

use std::time::Instant;

use madcawl_agent::kind;

use super::Proxy;
use crate::serve::netproc::{BRAKE_CRASHES, BRAKE_WINDOW};

/// The most page numbers remembered as lost to the crash brake.
const MAX_LOST: usize = 256;

impl Proxy<'_> {
    /// The content process ended (`how`): another is started and the
    /// pages opened in it again, unless the brake holds; what came of it.
    pub(super) fn recover(&mut self, how: &str) -> String {
        if let Some(mut c) = self.child.take() {
            c.reap();
        }
        self.log("crash", how);
        self.drop_registrations();
        for id in std::mem::take(&mut self.reserved) {
            self.release(id);
        }
        let now = Instant::now();
        self.crashes.push_back(now);
        while self.crashes.front().is_some_and(|&t| now - t > BRAKE_WINDOW) {
            self.crashes.pop_front();
        }
        if self.crashes.len() >= BRAKE_CRASHES {
            let why = format!("it ended {BRAKE_CRASHES} times within {} s", BRAKE_WINDOW.as_secs());
            return self.drop_pages(&why);
        }
        if let Err(e) = self.start() {
            return self.drop_pages(&format!("a new one could not start: {e}"));
        }
        self.restarts += 1;
        let mut done = Vec::new();
        for p in std::mem::take(&mut self.pages) {
            match self.ask(&p.restore_frame(), self.shared.config.hang) {
                Ok(r) if r.kind == kind::RESULT => {
                    done.push(format!("p{} at y={}", p.id, p.y));
                    self.pages.push(p);
                }
                _ => {
                    done.push(format!("p{} could not be opened again", p.id));
                    self.lose(p.id);
                }
            }
        }
        self.log("recovered", &format!("{} pages", self.pages.len()));
        match done.is_empty() {
            true => "a new content process was started".into(),
            false => format!("a new content process was started and the pages opened again: {}", done.join(", ")),
        }
    }

    /// Drops every page after the brake held or a restart failed.
    fn drop_pages(&mut self, why: &str) -> String {
        let gone: Vec<u32> = self.pages.drain(..).map(|p| p.id).collect();
        for &id in &gone {
            self.lose(id);
        }
        self.log("crash-brake", why);
        let names: Vec<String> = gone.iter().map(|id| format!("p{id}")).collect();
        format!("no new content process was started ({why}); dropped {}; the next request starts one", names.join(" "))
    }

    /// Gives back a page that could not be kept; requests on it are told
    /// so from now on.
    fn lose(&mut self, id: u32) {
        self.release(id);
        if self.lost.len() == MAX_LOST {
            self.lost.pop_front();
        }
        self.lost.push_back(id);
    }
}
