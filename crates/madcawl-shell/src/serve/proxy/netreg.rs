//! What a content process may have of the network service: a
//! registration the host makes in the session's own cookie partition,
//! and the release of one the host made for the session. Nothing it
//! sends reaches the control channel as sent, so it cannot name another
//! session's partition or the profile's, drop a partition, or read the
//! service's counts. A new network process starts with every jar empty;
//! the session's next reply says so.

use madcawl_agent::{Frame, kind, tag};

use super::Proxy;
use crate::serve::{code, error_frame};

/// Registrations a session may hold for each page it may open: one a
/// page, with room for those a navigation makes before the last is
/// released.
const TOKENS_PER_PAGE: usize = 4;

const JAR_LOST: &str = "note: the network process was restarted, and this session's cookies went with it; sites it was logged in to need logging in again";

impl Proxy<'_> {
    pub(super) fn net_register(&mut self) -> Frame {
        let Some(net) = self.shared.net.clone() else { return error_frame(code::REFUSED, "this host has no network") };
        let generation = net.generation();
        self.tokens.retain(|_, g| *g == generation);
        if self.tokens.len() >= self.shared.config.max_pages * TOKENS_PER_PAGE {
            self.log("net-register", "error 13");
            return error_frame(code::TOO_MANY_PAGES, "the session holds as many network registrations as it may");
        }
        match net.register(self.id) {
            Ok((token, g)) => {
                self.saw_jar(g);
                let r = Frame::new(kind::RESULT).bytes(tag::DATA, &token);
                self.tokens.insert(token, g);
                r
            }
            Err(e) => error_frame(code::IO, &format!("the network service: {e}")),
        }
    }

    pub(super) fn net_release(&mut self, q: &Frame) -> Frame {
        let (Some(net), Some(t)) = (self.shared.net.clone(), q.field(tag::DATA)) else {
            return error_frame(code::BAD_REQUEST, "no registration named");
        };
        if self.tokens.remove(t).is_none() {
            self.log("net-release", "error 11");
            return error_frame(code::REFUSED, "not a registration the host made for this session");
        }
        match net.unregister(t) {
            Ok(_) => Frame::new(kind::RESULT),
            Err(e) => error_frame(code::IO, &format!("the network service: {e}")),
        }
    }

    /// Drops the session's registrations, as when its content process
    /// ended; a network process that has ended took them with it.
    pub(super) fn drop_registrations(&mut self) {
        let Some(net) = self.shared.net.clone() else { return };
        for (t, _) in self.tokens.drain() {
            let _ = net.unregister(&t);
        }
    }

    /// The session's cookies are in network process `generation` now.
    fn saw_jar(&mut self, generation: u64) {
        if self.jar.is_some_and(|g| g != generation) {
            self.jar_lost = true;
        }
        self.jar = Some(generation);
    }

    /// `reply`, with a line first saying the session's cookies were lost
    /// when the network process holding them ended since the last reply.
    pub(super) fn jar_note(&mut self, mut reply: Frame) -> Frame {
        if let (Some(g), Some(net)) = (self.jar, &self.shared.net)
            && net.generation() != g
        {
            self.jar_lost = true;
            self.jar = None;
        }
        if !std::mem::take(&mut self.jar_lost) {
            return reply;
        }
        self.log("cookies-lost", "netd restarted");
        match reply.fields.iter_mut().find(|(t, _)| *t == tag::TEXT) {
            Some((_, text)) => {
                let mut v = format!("{JAR_LOST}\n").into_bytes();
                v.append(text);
                *text = v;
            }
            None => reply = reply.str(tag::TEXT, JAR_LOST),
        }
        reply
    }

    /// An audit line from the content process, written under the
    /// session's number whatever it says.
    pub(super) fn audit(&self, q: &Frame) {
        let f: Vec<&str> = q.fields.iter().filter(|(t, _)| *t == tag::FIELD).map(|(_, v)| std::str::from_utf8(v).unwrap_or("?")).collect();
        let [origin, action, role, name, result] = f[..] else {
            self.log("audit", "a malformed line from the content process");
            return;
        };
        let line = crate::serve::log::Line { session: self.id, origin, action, role, name, result };
        self.shared.log.write(&line);
    }
}
