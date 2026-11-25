//! The host's half of the guard: registrations made, moved and dropped,
//! cookie partitions dropped, and refusal counts read, on the control
//! channel.

use super::{Doc, Guard, Token, new_token, partition, token_of};
use crate::error;
use madcawl_net::Url;
use madcawl_net::partition::PROFILE;
use madcawl_net::wire::{Frame, kind, tag};
use std::collections::HashMap;
use std::io;
use std::os::unix::net::UnixStream;

impl Guard {
    /// Registers a document, or moves the registration `token` to another
    /// URL in the same partition.
    fn register(&self, docs: &mut HashMap<Token, Doc>, f: &Frame, id: u64, token: Option<Token>) -> Frame {
        let host = f.get_num(tag::NAVIGATION).is_some_and(|n| n != 0);
        let url = f.get_str(tag::URL).and_then(|s| Url::parse(s).ok());
        if url.is_none() && !host {
            return error(id, "missing or bad url");
        }
        let named = f.field(tag::PARTITION).is_some();
        let Ok(part) = partition::of_register(f) else { return error(id, "bad partition") };
        let done = |t: &Token| Frame::new(kind::REGISTERED).num(tag::ID, id).bytes(tag::TOKEN, t);
        let Some(t) = token else {
            if docs.len() >= self.max {
                return error(id, "too many registered documents");
            }
            if part != PROFILE && let Err(e) = self.client.partitions().open(part) {
                return error(id, &e.to_string());
            }
            let t = new_token();
            docs.insert(t, Doc { url: if host { None } else { url }, partition: part, refused: 0, logged: 0 });
            return done(&t);
        };
        match docs.get_mut(&t) {
            Some(d) if named && d.partition != part => error(id, "a registration stays in its cookie partition"),
            Some(d) if d.url.is_some() && !host => {
                d.url = url;
                done(&t)
            }
            Some(_) => error(id, "the host's own registration has no document"),
            None => error(id, "unknown token"),
        }
    }

    /// Drops a partition's cookies and every registration in it.
    fn drop_partition(&self, docs: &mut HashMap<Token, Doc>, f: &Frame, id: u64) -> Frame {
        let Some(part) = f.get_num(tag::PARTITION).filter(|&p| p != PROFILE) else {
            return error(id, "missing partition, or the profile's own");
        };
        let before = docs.len();
        docs.retain(|_, d| d.partition != part);
        let open = self.client.partitions().close(part);
        let dropped = (before - docs.len()) as u64;
        Frame::new(kind::PARTITION_DROP).num(tag::ID, id).num(tag::OK, u64::from(open)).num(tag::COUNT, dropped)
    }

    /// Answers one frame from the host; `None` for a kind it does not know.
    pub(super) fn control(&self, f: &Frame) -> Option<Frame> {
        let id = f.get_num(tag::ID).unwrap_or(0);
        if !matches!(f.kind, kind::REGISTER | kind::UNREGISTER | kind::STATS | kind::PARTITION_DROP) {
            return None;
        }
        let Ok(token) = token_of(f) else { return Some(error(id, "bad token")) };
        let mut docs = self.lock();
        let done = |t: &Token| Frame::new(kind::REGISTERED).num(tag::ID, id).bytes(tag::TOKEN, t);
        Some(match (f.kind, token) {
            (kind::REGISTER, _) => self.register(&mut docs, f, id, token),
            (kind::PARTITION_DROP, _) => self.drop_partition(&mut docs, f, id),
            (kind::UNREGISTER, Some(t)) if docs.remove(&t).is_some() => done(&t),
            (kind::UNREGISTER, _) => error(id, "unknown token"),
            (_, Some(t)) => match docs.get(&t) {
                Some(d) => Frame::new(kind::STATS).num(tag::ID, id).num(tag::COUNT, d.refused),
                None => error(id, "unknown token"),
            },
            (_, None) => Frame::new(kind::STATS).num(tag::ID, id).num(tag::COUNT, self.refused()),
        })
    }

    /// Serves the host's control channel until the host closes it; the
    /// registrations stay as they are.
    pub fn serve_control(&self, mut control: UnixStream) -> io::Result<()> {
        while let Some(frame) = Frame::read_from(&mut control)? {
            if let Some(reply) = self.control(&frame) {
                reply.write_to(&mut control)?;
            }
        }
        Ok(())
    }
}
