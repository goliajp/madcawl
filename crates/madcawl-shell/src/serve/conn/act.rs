//! A session's actions, waits and request logs: the page checked to be
//! the session's own (another session's is refused and the refusal
//! audited), the action carried out and waited on, and the reply — what
//! the action did, whether the page is stable and what is still missing,
//! then the diff from before the action, or the new document's outline
//! after a navigation — in an envelope, with the time each part took.

use std::time::Instant;

use madcawl_agent::act::{Action, Until};
use madcawl_agent::{Frame, kind, tag};

use super::Conn;
use crate::serve::link::internal;
use crate::serve::act::ActError;
use crate::serve::doc::layout_time;
use crate::serve::page::{Env, Page};
use crate::serve::{Shared, code, error_frame};

/// Request log lines a reply gives when the request names no limit.
const DEFAULT_REQUESTS: usize = 50;

pub(crate) fn env(shared: &Shared, owner: u32) -> Env<'_> {
    let c = &shared.config;
    Env { allow: &c.allow, net: shared.net.as_ref(), uploads: &c.uploads, approve: &c.approve, owner, run: &shared.run }
}

fn page_gen(p: &Page) -> u32 {
    p.agents.page_gen()
}

fn us(from: Instant, to: Instant) -> u64 {
    to.duration_since(from).as_micros() as u64
}

fn failure(e: ActError) -> Frame {
    match e {
        ActError::Check(m) => error_frame(code::CHECK, &m),
        ActError::Agent(e) => {
            let c = match e {
                madcawl_agent::AgentError::Ref(madcawl_agent::RefError::Stale) => 3,
                madcawl_agent::AgentError::Ref(_) => 2,
                _ => code::BAD_REQUEST,
            };
            error_frame(c, e.message())
        }
        ActError::Refused(c, m) => error_frame(c, &m),
        ActError::Bad(m) => error_frame(code::BAD_REQUEST, &m),
    }
}

impl Conn<'_> {
    /// The index of the session's page the frame names; another
    /// session's page is refused, and that is audited.
    pub(super) fn page_for(&self, f: &Frame, action: &str) -> Result<usize, Frame> {
        if let Some(i) = self.page_index(f) {
            return Ok(i);
        }
        let id = f.get_num(tag::PAGE).unwrap_or(0) as u32;
        let theirs = self.shared.owners.lock().unwrap_or_else(|e| e.into_inner()).get(&id).is_some_and(|&s| s != self.id)
            || self.shared.link.is_some() && f.get_num(internal::FOREIGN) == Some(1);
        if !theirs {
            return Err(self.no_page(action));
        }
        let r = error_frame(code::DENIED, "this agent session may not touch that page: another session opened it");
        self.log("-", &format!("refused {action}"), ("", ""), &r);
        Err(r)
    }

    /// An `ACT` or a `WAIT`.
    pub(super) fn act(&mut self, f: &Frame) -> Frame {
        let t0 = Instant::now();
        let (action, until, name) = match request(f) {
            Ok(r) => r,
            Err(r) => {
                self.log("-", "act", ("", ""), &r);
                return r;
            }
        };
        let i = match self.page_for(f, &name) {
            Ok(i) => i,
            Err(r) => return r,
        };
        let env = env(self.shared, self.id);
        let p = &mut self.pages[i];
        p.last_used = t0;
        let target = action.as_ref().and_then(|a| a.target()).and_then(|r| p.target(r));
        let since = f.get_num(tag::SINCE).filter(|_| action.is_none());
        let out = run(p, action.as_ref(), &until, since, t0, &env);
        let url = p.url.clone();
        let reply = match out {
            Ok(r) => self.wrap(r, &url),
            Err(r) => r,
        };
        let (role, nm) = target.unwrap_or(("", String::new()));
        self.log(&url, &name, (role, &nm), &reply);
        reply
    }

    /// A page's request log, load errors and script notes.
    pub(super) fn requests(&mut self, f: &Frame) -> Frame {
        let i = match self.page_for(f, "requests") {
            Ok(i) => i,
            Err(r) => return r,
        };
        let p = &mut self.pages[i];
        p.last_used = Instant::now();
        let limit = f.get_num(tag::LIMIT).map_or(DEFAULT_REQUESTS, |l| l as usize);
        let text = p.log.text(limit, &p.loader.lines());
        let url = p.url.clone();
        let reply = self.wrap(Frame::new(kind::RESULT).str(tag::TEXT, &text), &url);
        self.log(&url, "requests", ("", ""), &reply);
        reply
    }
}

/// The action (none for a wait), the wait and the audit log's name.
fn request(f: &Frame) -> Result<(Option<Action>, Until, String), Frame> {
    let action = match f.kind {
        kind::ACT => Some(Action::parse(f).map_err(|e| error_frame(code::BAD_REQUEST, e))?),
        _ => None,
    };
    let until = Until::parse(f).map_err(|e| error_frame(code::BAD_REQUEST, e))?;
    let name = action.as_ref().map_or("wait".to_owned(), |a| format!("act:{}", a.name()));
    Ok((action, until, name))
}

/// Acts, waits and writes the reply: what the action did, whether the
/// page is stable and what is missing, then the diff since before it,
/// or the outline of the document a navigation brought.
fn run(p: &mut Page, action: Option<&Action>, until: &Until, since: Option<u64>, t0: Instant, env: &Env) -> Result<Frame, Frame> {
    let gen0 = page_gen(p);
    let laid0 = layout_time();
    // waiting for text asks geometry; laying out before the action keeps
    // the layout ahead of every change the action makes, which is what
    // lets the diff start past it (`Page::diff_floor`)
    if until.text.is_some() {
        p.geometry();
    }
    let since = since.unwrap_or_else(|| p.sync());
    let mut text = match action {
        Some(a) => p.act(a, env).map_err(failure)?,
        None => "waited".to_owned(),
    };
    let t1 = Instant::now();
    let waited = p.wait(until, t0, env);
    let t2 = Instant::now();
    text.push('\n');
    for n in &waited.notes {
        text.push_str(&format!("{n}\n"));
    }
    match waited.stable {
        true => text.push_str("stable\n"),
        false => text.push_str(&format!("not stable after {} ms\n", until.timeout_ms)),
    }
    for m in &waited.missing {
        text.push_str(&format!("missing: {m}\n"));
    }
    let changes = if page_gen(p) != gen0 {
        let outline = p.handle(&Frame::new(kind::READ).num(tag::VIEW, 0));
        format!("the page is now {}\n{}", p.url, outline.as_ref().and_then(|o| o.get_str(tag::TEXT)).unwrap_or(""))
    } else {
        let d = p.handle(&Frame::new(kind::DIFF).num(tag::SINCE, since));
        d.as_ref().and_then(|d| d.get_str(tag::TEXT)).unwrap_or("").to_owned()
    };
    text.push_str(&changes);
    let t3 = Instant::now();
    Ok(Frame::new(kind::RESULT)
        .num(tag::SEQ, p.agents.seq())
        .num(tag::STABLE, u64::from(waited.stable))
        .num(tag::DISPATCH_US, us(t0, t1))
        .num(tag::WAIT_US, us(t1, t2))
        .num(tag::DIFF_US, us(t2, t3))
        .num(tag::LAYOUT_US, (layout_time() - laid0).as_micros() as u64)
        .str(tag::TEXT, &text))
}

#[cfg(test)]
#[path = "lazy_tests.rs"]
mod tests;
