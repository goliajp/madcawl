//! One hop of a fetch: cookies stored from its response, and the final
//! response's head and decoded body handed to the requester's sink.

use super::{Client, Redirect, Request, Response, now_secs};
use crate::cookie::{Api, CookieJar};
use crate::decode::{self, Decoder};
use crate::error::NetError;
use crate::http1::{BodySink, RawHead};
use crate::partition;
use alt_url::Url;
use std::sync::Mutex;

/// Where a fetch's final response goes: its head (with an empty body),
/// then the decoded body in pieces.
pub trait Sink {
    fn head(&mut self, r: &Response) -> Result<(), NetError>;
    fn body(&mut self, chunk: &[u8]) -> Result<(), NetError>;
}

fn is_followed_redirect(head: &RawHead, mode: Redirect) -> bool {
    mode != Redirect::Manual && matches!(head.status, 301 | 302 | 303 | 307 | 308) && head.header("location").is_some()
}

/// One hop's response: cookies stored from every hop, the body of a
/// redirect that is followed dropped, the final one decoded into the sink.
pub(super) struct Hop<'a, 's> {
    pub client: &'a Client,
    /// the request's partition's jar
    pub jar: &'a Mutex<CookieJar>,
    pub req: &'a Request,
    pub url: &'a Url,
    pub url_list: &'a [Url],
    pub method: &'a str,
    pub sink: &'s mut dyn Sink,
    pub decoder: Option<Decoder>,
    pub delivered: bool,
}

impl BodySink for Hop<'_, '_> {
    fn head(&mut self, head: &RawHead) -> Result<bool, NetError> {
        if self.req.credentials {
            let mut jar = partition::lock(self.jar);
            let now = now_secs();
            for (_, v) in head.headers.iter().filter(|(n, _)| n.eq_ignore_ascii_case("set-cookie")) {
                jar.set(self.url, &String::from_utf8_lossy(v), Api::Http, now);
            }
        }
        if is_followed_redirect(head, self.req.redirect) {
            return Ok(false);
        }
        let has_body = self.method != "HEAD" && !matches!(head.status, 204 | 304);
        if has_body && let Some(enc) = head.header("content-encoding") {
            self.decoder = Some(Decoder::new(&decode::codings(enc)?, self.client.config.max_decoded));
        }
        let r = Response {
            status: head.status,
            reason: head.reason.clone(),
            url_list: self.url_list.to_vec(),
            headers: head.headers.clone(),
            body: Vec::new(),
        };
        self.sink.head(&r)?;
        self.delivered = true;
        Ok(true)
    }

    fn body(&mut self, bytes: &[u8]) -> Result<(), NetError> {
        let Hop { decoder, sink, .. } = self;
        match decoder {
            Some(d) => d.push(bytes, &mut |b| sink.body(b)),
            None => sink.body(bytes),
        }
    }
}
