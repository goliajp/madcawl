//! One agent session on a host with `--isolate`: the host keeps the
//! agent's connection and hands each request to a content process of
//! the session's own (`content`), so a crash, a hang or a memory cap
//! takes down that session's pages and nothing else of the host's.
//!
//! After every request the host asks the process where its pages are
//! (URL, viewport, how far scrolled). When the process ends, the host
//! starts another and opens those pages again under the same numbers,
//! scrolled to where they were; a request in flight gets an error saying
//! so. A process that does not answer within the hang time is killed
//! and recovered the same way. One that ends three times within a minute
//! is not started again: its pages are dropped, and named in the errors
//! to requests on them; the session's next request starts a new process.
//! What a page held beyond its URL and scroll position (history, typed
//! text) is not restored.
//!
//! A content process is not trusted with anything of the host's: it gets
//! page numbers only up to the session's limit, its word on which pages
//! it holds counts only for the session's own, and the network service
//! hears from it only through registrations the host makes in the
//! session's own cookie partition (`netreg`).

mod audit;
mod content;
mod netreg;
mod pages;
mod recover;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, VecDeque};
use std::os::fd::AsRawFd as _;
use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::time::Instant;

use madcawl_agent::{Frame, kind, tag};

use super::conn::PageState;
use super::link::internal;
use super::log::Line;
use super::wire::{Got, read_frame};
use super::{Shared, activation, code, error_frame};
use content::{Content, Fault};

pub(crate) fn run(id: u32, shared: &Shared, agent: UnixStream) {
    let mut p = Proxy::new(id, shared);
    p.serve(agent);
    p.end();
}

struct Proxy<'a> {
    id: u32,
    shared: &'a Shared,
    child: Option<Content>,
    /// The pages as the content process last told
    pages: Vec<PageState>,
    /// Numbers given out during the request in flight
    reserved: Vec<u32>,
    /// Pages dropped when the brake held
    lost: VecDeque<u32>,
    crashes: VecDeque<Instant>,
    restarts: u32,
    /// The session's registrations with the network service, each with
    /// the generation of the network process it was made in
    tokens: HashMap<Vec<u8>, u64>,
    /// The generation of the network process holding the session's
    /// cookies, once it has any
    jar: Option<u64>,
    /// The session's cookies went with a network process that ended, and
    /// its next reply has not said so yet
    jar_lost: bool,
    audit: audit::Budget,
}

impl<'a> Proxy<'a> {
    fn new(id: u32, shared: &'a Shared) -> Proxy<'a> {
        let (pages, reserved, tokens) = (Vec::new(), Vec::new(), HashMap::new());
        Proxy { id, shared, child: None, pages, reserved, lost: VecDeque::new(), crashes: VecDeque::new(), restarts: 0, tokens, jar: None, jar_lost: false, audit: audit::Budget::new() }
    }

    fn log(&self, action: &str, result: &str) {
        self.shared.log.write(&Line { session: self.id, origin: "-", action, role: "", name: "", result });
    }

    fn serve(&mut self, mut agent: UnixStream) {
        let max = self.shared.config.max_request;
        loop {
            let theirs = self.child.as_ref().map(Content::fd);
            // with no content process, the session ends after the idle
            // time, as a session with no pages does
            let wait = theirs.is_none().then_some(self.shared.config.idle);
            let Ok((from_agent, from_child)) = activation::readable_pair(agent.as_raw_fd(), theirs, wait) else { return };
            if !from_agent && !from_child && theirs.is_none() {
                self.log("idle", "ok");
                return;
            }
            // a process that ended is seen to before a request goes to it
            if from_child && !self.child_spoke() {
                return;
            }
            if !from_agent {
                continue;
            }
            let (reply, last) = match read_frame(&mut agent, max, None) {
                Got::Frame(f) => (self.forward(&f), false),
                Got::Closed => return,
                Got::Idle => continue,
                Got::TooLong(n) => (error_frame(code::REQUEST_TOO_LARGE, &format!("a request of {n} bytes is more than the {max} one may be")), true),
                Got::Bad => (error_frame(code::BAD_REQUEST, "a malformed frame"), true),
            };
            if last {
                self.log("frame", &format!("error {}", reply.get_num(tag::CODE).unwrap_or(0)));
            }
            let mut out = Vec::new();
            reply.encode(&mut out);
            if std::io::Write::write_all(&mut agent, &out).is_err() || last {
                return;
            }
        }
    }

    /// The content process said something, or ended, while no request
    /// was open; whether the session goes on. One that ended well ended
    /// the session (idle with no pages); one that crashed is recovered.
    fn child_spoke(&mut self) -> bool {
        let Some(mut c) = self.child.take() else { return true };
        let ended = c.idle_read(&mut |q| self.answer(q));
        let well = ended.is_some() && c.ended_well();
        self.child = (!well).then_some(c);
        let Some(how) = ended else { return true };
        if well {
            return false;
        }
        let note = self.recover(&how);
        eprintln!("madcawl-shell: session {}: the content process ended while idle ({how}); {note}", self.id);
        true
    }

    fn forward(&mut self, f: &Frame) -> Frame {
        if internal::is_internal(f.kind) {
            let r = error_frame(code::UNKNOWN, "unknown request kind");
            self.log(&format!("kind-{}", f.kind), "error 6");
            return r;
        }
        if let Some(r) = self.lost(f) {
            return r;
        }
        let f = &self.mark_foreign(f);
        if self.child.is_none()
            && let Err(e) = self.start()
        {
            return error_frame(code::IO, &format!("the host cannot start a content process: {e}"));
        }
        let hang = self.shared.config.hang;
        let reply = match self.ask(f, hang) {
            Ok(r) => r,
            Err(fault) => {
                let hung = matches!(fault, Fault::Hung);
                let how = self.ended(fault);
                let note = self.recover(&how);
                match hung {
                    true => error_frame(code::HUNG, &format!("the session's content process did not answer within {} s; {note}", hang.as_secs())),
                    false => error_frame(code::CRASHED, &format!("the session's content process ended while answering: {how}; {note}")),
                }
            }
        };
        let reply = if f.kind == kind::STATS && reply.kind == kind::RESULT { self.with_counters(reply) } else { reply };
        self.refresh();
        self.jar_note(reply)
    }

    /// How the content process ended; one that did not answer in time is
    /// killed first, so nothing it says later is taken for an answer.
    fn ended(&mut self, fault: Fault) -> String {
        match fault {
            Fault::Ended(how) => how,
            Fault::Hung => {
                let how = self.child.as_mut().map(Content::kill).unwrap_or_default();
                format!("it did not answer within {} s and was killed ({how})", self.shared.config.hang.as_secs())
            }
        }
    }

    /// The refusal of a request on a page lost to the crash brake.
    fn lost(&self, f: &Frame) -> Option<Frame> {
        let page = f.get_num(tag::PAGE)? as u32;
        if !self.lost.contains(&page) {
            return None;
        }
        let msg = format!("p{page} was dropped: the session's content process kept crashing; open the page again");
        self.log("lost", "error 24");
        Some(error_frame(code::CRASHED, &msg))
    }

    /// `f` without the host's own tag, which is set when the page it names
    /// is another session's: the content process refuses it and audits
    /// the refusal as a host without `--isolate` does.
    fn mark_foreign(&self, f: &Frame) -> Frame {
        let mut f = f.clone();
        f.fields.retain(|(t, _)| *t != internal::FOREIGN);
        let page = f.get_num(tag::PAGE).map(|p| p as u32);
        let owner = page.and_then(|p| self.shared.owners.lock().unwrap_or_else(|e| e.into_inner()).get(&p).copied());
        match owner.is_some_and(|o| o != self.id) {
            true => f.num(internal::FOREIGN, 1),
            false => f,
        }
    }

    fn start(&mut self) -> std::io::Result<()> {
        let exe = self.shared.config.isolate.as_deref().expect("a proxy runs only with --isolate");
        let args = super::content::args(&self.shared.config, self.id, &self.shared.run);
        self.child = Some(Content::start(exe, &args)?);
        Ok(())
    }

    /// `f` asked of the content process, its questions answered here.
    fn ask(&mut self, f: &Frame, wait: std::time::Duration) -> Result<Frame, Fault> {
        let mut c = self.child.take().ok_or(Fault::Ended("no content process".into()))?;
        let r = c.ask(f, wait, &mut |q| self.answer(q));
        self.child = Some(c);
        r
    }

    /// A question from the content process, and the answer when it takes
    /// one; any question but these few is refused.
    fn answer(&mut self, q: &Frame) -> Option<Frame> {
        Some(match q.kind {
            internal::NEW_PAGE => self.new_page(),
            internal::NET_REGISTER => self.net_register(),
            internal::NET_RELEASE => self.net_release(q),
            internal::AUDIT => {
                self.audit(q);
                return None;
            }
            k => {
                self.content_log(&format!("question-{k}"), "error 11");
                error_frame(code::REFUSED, "the host does not take that question from a content process")
            }
        })
    }

    /// A `STATS` answer with the host's counters for the session added.
    fn with_counters(&self, mut r: Frame) -> Frame {
        let Some(at) = r.fields.iter().position(|(t, _)| *t == tag::TEXT) else { return r };
        let mut text = String::from_utf8_lossy(&r.fields[at].1).into_owned();
        let pid = self.child.as_ref().map_or(0, Content::pid);
        let pages = self.shared.pages.load(Ordering::SeqCst);
        text.push_str(&format!("host-pid {}\nhost-pages {pages}\ncontent-pid {pid}\ncontent-restarts {}\n", std::process::id(), self.restarts));
        if let Some(p) = self.shared.net.as_ref().and_then(|n| n.process.as_ref()) {
            text.push_str(&format!("netd-pid {}\nnetd-restarts {}\n", p.pid(), p.restarts.load(Ordering::SeqCst)));
        }
        r.fields[at].1 = text.into_bytes();
        r
    }

    /// The session ended: the content process closes its pages and exits,
    /// and what the host holds for the session goes.
    fn end(&mut self) {
        if let Some(c) = self.child.take() {
            c.finish(&mut |q| self.answer(q));
        }
        self.audit_dropped();
        let ids: Vec<u32> = self.pages.drain(..).map(|p| p.id).chain(self.reserved.drain(..)).collect();
        for id in ids {
            self.release(id);
        }
        // dropping the partition drops its registrations with it
        self.forget_registrations();
        if let Some(net) = &self.shared.net
            && let Err(e) = net.end_session(self.id)
        {
            eprintln!("madcawl-shell: session {}: dropping its cookies: {e}", self.id);
        }
    }
}
