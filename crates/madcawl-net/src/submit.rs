//! The FETCH frame of a navigation, a form's submission among them:
//! method, URL, the body and its type, and the document it came from,
//! which the request's `Origin` and cookies' site context are decided by.
//! A content process only asks for a navigation; its host sends this frame
//! on the host's own connection to the network process, which refuses a
//! navigation from a content process.

use crate::Url;
use crate::wire::{Frame, kind, tag};

pub struct Navigation<'a> {
    pub id: u64,
    pub method: &'a str,
    pub url: &'a Url,
    pub content_type: Option<&'a str>,
    pub body: Option<&'a [u8]>,
    pub initiator: Option<&'a Url>,
}

pub fn fetch_frame(n: &Navigation) -> Frame {
    let mut f = Frame::new(kind::FETCH).num(tag::ID, n.id).str(tag::METHOD, n.method).str(tag::URL, n.url.as_str()).num(tag::NAVIGATION, 1);
    if let Some(i) = n.initiator {
        f = f.str(tag::INITIATOR, i.as_str());
    }
    if let Some(ct) = n.content_type {
        f = f.header("content-type", ct.as_bytes());
    }
    if let Some(b) = n.body {
        f = f.bytes(tag::BODY, b);
    }
    f
}
