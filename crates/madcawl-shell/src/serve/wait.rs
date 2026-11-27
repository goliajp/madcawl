//! Waiting for a page to be stable, decided by the page's own
//! state and never by sleeping: no navigation in flight, the parser done
//! with its input, no request in flight for the page, its styles up to
//! date; and, when asked, a reference that names a node or
//! a text that is on the page. A wait that runs out says what is still
//! missing. Without script nothing changes a stable page, so a condition
//! not met once the page is stable is reported at once, not waited for.

use std::time::Instant;

use madcawl_agent::act::Until;

use super::loader::Stage;
use super::page::{Env, Page};

pub(crate) struct Waited {
    pub stable: bool,
    /// What a finished load came to
    pub notes: Vec<String>,
    /// What was still missing, a line each
    pub missing: Vec<String>,
}

impl Page {
    pub(crate) fn wait(&mut self, until: &Until, started: Instant, env: &Env) -> Waited {
        let deadline = started + std::time::Duration::from_millis(until.timeout_ms);
        let mut notes = Vec::new();
        while !self.loader.is_idle() {
            if let Some(n) = self.settle(deadline, env) {
                notes.push(n);
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        let mut missing = self.unsettled();
        let stable = missing.is_empty();
        if stable {
            missing.extend(self.unmet(until));
        }
        Waited { stable, notes, missing }
    }

    /// The conditions of stability not met, a line each.
    fn unsettled(&self) -> Vec<String> {
        let mut out = Vec::new();
        match &self.loader.stage {
            Stage::Document(..) => out.push("a navigation is in flight".to_owned()),
            Stage::Scripts(_) => out.push("the new document's scripts run; its window has not loaded yet".to_owned()),
            Stage::Sheets(_) => {
                out.push("the new document is parsed; its styles and layout wait for its stylesheets".to_owned());
            }
            Stage::Idle => {}
        }
        let lines = self.loader.lines();
        if !lines.is_empty() {
            out.push(format!("{} requests in flight: {}", lines.len(), lines.join("; ")));
        }
        out
    }

    /// The conditions the caller added, not met.
    fn unmet(&mut self, until: &Until) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(r) = &until.reference
            && let Err(e) = self.resolve(r)
        {
            out.push(format!("[{r}] names no node ({}); the page is stable and will not change without script", e.message()));
        }
        if let Some(t) = &until.text
            && !self.has_text(t)
        {
            out.push(format!("the text {t:?} is not on the page; the page is stable and will not change without script"));
        }
        out
    }

    /// Some shown node's name or value holds `t`, ignoring case.
    fn has_text(&mut self, t: &str) -> bool {
        // whether text is shown takes geometry: clipped away or off the
        // canvas, it is not
        self.geometry();
        let (dom, inputs) = self.doc.parts();
        self.agents.sync(dom, inputs);
        let want = t.to_lowercase();
        let Some(tree) = self.agents.tree() else { return false };
        let dom = &self.doc.dom;
        dom.descendants(dom.document()).any(|n| {
            tree.record(n).is_some_and(|r| {
                r.shown() && (tree.name(r).to_lowercase().contains(&want) || tree.value(r).to_lowercase().contains(&want))
            })
        })
    }
}
