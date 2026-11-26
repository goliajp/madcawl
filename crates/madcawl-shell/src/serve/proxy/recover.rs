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
    /// A page whose restore the new process does not answer, or ends on,
    /// is dropped and that process is recovered in turn, so every round
    /// has a page fewer to open and the rounds count toward the brake.
    pub(super) fn recover(&mut self, how: &str) -> String {
        let mut how = how.to_owned();
        let mut failed = Vec::new();
        loop {
            if let Some(why) = self.restart(&how) {
                return self.drop_pages(&why);
            }
            match self.restore(&mut failed) {
                Ok(opened) => {
                    self.log("recovered", &format!("{} pages", self.pages.len()));
                    let done: Vec<String> = opened.into_iter().chain(failed).collect();
                    return match done.is_empty() {
                        true => "a new content process was started".into(),
                        false => format!("a new content process was started and the pages opened again: {}", done.join(", ")),
                    };
                }
                Err(next) => how = next,
            }
        }
    }

    /// Ends what is left of the content process and starts another,
    /// unless the brake holds; why not, when it was not started.
    fn restart(&mut self, how: &str) -> Option<String> {
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
            return Some(format!("it ended {BRAKE_CRASHES} times within {} s", BRAKE_WINDOW.as_secs()));
        }
        if let Err(e) = self.start() {
            return Some(format!("a new one could not start: {e}"));
        }
        self.restarts += 1;
        None
    }

    /// Opens the pages again in the new process: where each is, or, in
    /// `failed`, that it could not be. When the process does not answer
    /// or ends, the page it was opening is dropped and the error says how
    /// the process ended (killed first when it hung); the pages not yet
    /// opened are kept for the next process.
    fn restore(&mut self, failed: &mut Vec<String>) -> Result<Vec<String>, String> {
        let mut opened = Vec::new();
        let mut pending = std::mem::take(&mut self.pages).into_iter();
        while let Some(p) = pending.next() {
            match self.ask(&p.restore_frame(), self.shared.config.hang) {
                Ok(r) if r.kind == kind::RESULT => {
                    opened.push(format!("p{} at y={}", p.id, p.y));
                    self.pages.push(p);
                }
                Ok(_) => {
                    failed.push(format!("p{} could not be opened again", p.id));
                    self.lose(p.id);
                }
                Err(fault) => {
                    let how = self.ended(fault);
                    failed.push(format!("p{} could not be opened again ({how})", p.id));
                    self.lose(p.id);
                    self.pages.extend(pending);
                    return Err(format!("{how}, opening p{} again", p.id));
                }
            }
        }
        Ok(opened)
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
