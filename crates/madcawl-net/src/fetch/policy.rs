//! What each hop of a request may carry, decided from its initiator: the
//! site context its cookies are read in, whether cookies go with it at
//! all (its credentials mode), and its mode's rules. In CORS mode a hop
//! to another origin than the initiator's carries `Origin` and must be a
//! request that needs no preflight, since none is made; in same-origin
//! mode it is refused.

use super::Request;
use crate::cookie::{SiteContext, site_of};
use crate::error::NetError;
use alt_url::{Origin, Url};
use std::time::Instant;

/// Whether cookies go with a request and are stored from its responses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Credentials {
    Omit,
    /// only on hops of the initiator's origin
    SameOrigin,
    #[default]
    Include,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// no rules on where it goes: navigations, and what a page does not read
    #[default]
    NoCors,
    /// a response the page reads, checked by the requester against CORS
    Cors,
    /// only to the initiator's origin
    SameOrigin,
}

fn host_of(url: &Url) -> &str {
    url.host_str().unwrap_or("")
}

fn same_origin(a: &Origin, b: &Origin) -> bool {
    a.is_tuple() && b.is_tuple() && a.ascii_serialization() == b.ascii_serialization()
}

/// The origin requests are made for: the initiator's, or for the user
/// (no initiator) the request's own URL's.
fn origin_of(req: &Request) -> Origin {
    req.initiator.as_ref().unwrap_or(&req.url).origin()
}

pub(super) fn site_context(req: &Request, url: &Url, method: &str) -> SiteContext {
    let Some(init) = &req.initiator else { return SiteContext::SameSite };
    if init.scheme() == url.scheme() && site_of(host_of(init)) == site_of(host_of(url)) {
        SiteContext::SameSite
    } else if req.navigation {
        SiteContext::CrossSiteNavigation { safe: matches!(method, "GET" | "HEAD") }
    } else {
        SiteContext::CrossSite
    }
}

/// Whether the hop to `url` sends and stores cookies.
pub(super) fn with_cookies(req: &Request, url: &Url) -> bool {
    match req.credentials {
        Credentials::Omit => false,
        Credentials::SameOrigin => same_origin(&origin_of(req), &url.origin()),
        Credentials::Include => true,
    }
}

/// A hop to `url` can be made: of HTTP(S), before the request is
/// cancelled and before its deadline.
pub(super) fn may_go(req: &Request, url: &Url, deadline: Instant) -> Result<(), NetError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(NetError::Scheme(url.scheme().to_owned()));
    }
    if req.cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
        return Err(NetError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(NetError::Deadline);
    }
    Ok(())
}

/// The hop to `url` refused by the request's mode.
pub(super) fn check_hop(req: &Request, url: &Url, method: &str, headers: &[(String, Vec<u8>)]) -> Result<(), NetError> {
    if same_origin(&origin_of(req), &url.origin()) {
        return Ok(());
    }
    match req.mode {
        Mode::SameOrigin => Err(NetError::Refused(format!("a same-origin request going to {}", url.origin().ascii_serialization()))),
        Mode::Cors if !is_simple(method, headers) => Err(NetError::Refused("a cross-origin request that needs a preflight".into())),
        _ => Ok(()),
    }
}

/// The `Origin` header of the hop to `url`: on a method other than GET
/// and HEAD, and on a CORS request to another origin; "null" for an
/// opaque initiator and once a redirect tainted the request.
pub(super) fn origin_header(req: &Request, url: &Url, method: &str, tainted: bool) -> Option<String> {
    let origin = origin_of(req);
    let cors = req.mode == Mode::Cors && !same_origin(&origin, &url.origin());
    let send = cors || !matches!(method, "GET" | "HEAD");
    send.then(|| match &req.initiator {
        Some(i) if !tainted => i.origin().ascii_serialization(),
        _ => "null".into(),
    })
}

/// A redirect from `from` to `to` taints the request's origin when it
/// leaves an origin that was not the initiator's already.
pub(super) fn taints(req: &Request, from: &Url, to: &Url) -> bool {
    !same_origin(&from.origin(), &to.origin()) && !same_origin(&origin_of(req), &from.origin())
}

/// A request another origin may get without a preflight: GET, HEAD or
/// POST with CORS-safelisted headers only.
pub fn is_simple(method: &str, headers: &[(String, Vec<u8>)]) -> bool {
    matches!(method, "GET" | "HEAD" | "POST") && headers.iter().all(|(n, v)| is_safelisted(n, v))
}

/// A CORS-safelisted request header.
pub fn is_safelisted(name: &str, value: &[u8]) -> bool {
    if value.len() > 128 {
        return false;
    }
    let unsafe_byte = |c: &u8| (*c < 0x20 && *c != b'\t') || b"\"():<>?@[\\]{}\x7f".contains(c);
    match name.to_ascii_lowercase().as_str() {
        "accept" => !value.iter().any(unsafe_byte),
        "accept-language" | "content-language" => value.iter().all(|&c| c.is_ascii_alphanumeric() || b" *,-.;=".contains(&c)),
        "content-type" => {
            let essence = String::from_utf8_lossy(value.split(|&c| c == b';').next().unwrap_or_default()).trim().to_ascii_lowercase();
            !value.iter().any(unsafe_byte) && matches!(essence.as_str(), "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain")
        }
        _ => false,
    }
}
