//! What a content process may have of the network service: a
//! registration the host makes in the session's own cookie partition,
//! and the release of one the host made for the session. Nothing it
//! sends reaches the control channel as sent, so it cannot name another
//! session's partition or the profile's, drop a partition, or read the
//! service's counts. A new network process starts with every jar empty;
//! the session's next reply says so.
//!
//! The service holds a limited number of registrations across the host
//! (`Net::slot_limit`), so a session gets a fair share of them: the limit
//! divided among the sessions the host may have, or one more than the
//! pages it has open when that is more, since its requests come one at a
//! time and so at most one navigation's registration is made before the
//! last is released.

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
        if self.tokens.len() >= self.token_share(net.slot_limit()) {
            self.log("net-register", "error 13");
            return error_frame(code::TOO_MANY_PAGES, "the session holds as many network registrations as it may");
        }
        if !net.take_slot() {
            self.log("net-register", "error 13 host");
            return error_frame(code::TOO_MANY_PAGES, "the host holds as many network registrations as the network service takes");
        }
        match net.register(self.id) {
            Ok((token, g)) => {
                self.saw_jar(g);
                let r = Frame::new(kind::RESULT).bytes(tag::DATA, &token);
                self.tokens.insert(token, g);
                r
            }
            Err(e) => {
                net.give_slots(1, generation);
                error_frame(code::IO, &format!("the network service: {e}"))
            }
        }
    }

    /// How many registrations the session may hold of the `limit` the
    /// host may.
    fn token_share(&self, limit: usize) -> usize {
        let c = &self.shared.config;
        let fair = limit / c.max_sessions.max(1);
        let open = self.pages.len() + self.reserved.len();
        fair.max(open + 1).min(c.max_pages * TOKENS_PER_PAGE)
    }

    pub(super) fn net_release(&mut self, q: &Frame) -> Frame {
        let (Some(net), Some(t)) = (self.shared.net.clone(), q.field(tag::DATA)) else {
            return error_frame(code::BAD_REQUEST, "no registration named");
        };
        let Some(g) = self.tokens.remove(t) else {
            self.log("net-release", "error 11");
            return error_frame(code::REFUSED, "not a registration the host made for this session");
        };
        net.give_slots(1, g);
        match net.unregister(t) {
            Ok(_) => Frame::new(kind::RESULT),
            Err(e) => error_frame(code::IO, &format!("the network service: {e}")),
        }
    }

    /// Drops the session's registrations, as when its content process
    /// ended; a network process that has ended took them with it.
    pub(super) fn drop_registrations(&mut self) {
        let Some(net) = self.shared.net.clone() else { return };
        for (t, g) in self.tokens.drain() {
            net.give_slots(1, g);
            let _ = net.unregister(&t);
        }
    }

    /// Forgets the session's registrations once the service dropped them
    /// with its partition.
    pub(super) fn forget_registrations(&mut self) {
        let Some(net) = self.shared.net.clone() else { return };
        for (_, g) in self.tokens.drain() {
            net.give_slots(1, g);
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
}
