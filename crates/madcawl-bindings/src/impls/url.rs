//! The URL Standard's URL class over the network stack's URL parser:
//! the parts as the standard serializes them, the setters of
//! `url_parts.rs`, and `searchParams`, made when first read and kept in
//! step with the query both ways. `createObjectURL` and
//! `revokeObjectURL` work on the blob URL store (`blob_urls.rs`).

use alt_url::Url;
use madcawl_js::{Engine, ErrorKind};
use madcawl_str::DomString;

use crate::convert::{Thrown, throw, throw_dom, usv_string};
use crate::event::{Held, Target};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};
use crate::url_parts as parts;

pub(crate) struct UrlState {
    pub url: Url,
    /// `searchParams` once read, by a count; it holds the URL in turn.
    pub params: Option<Held>,
}

fn state<E: Engine>(h: &mut DomHost<E>, r: SideRef) -> &mut UrlState {
    match &mut h.sides.get_mut(r).data {
        SideData::Url(u) => u,
        _ => unreachable!("a URL's side object"),
    }
}

fn url<E: Engine>(h: &mut DomHost<E>, r: SideRef) -> &Url {
    &state(h, r).url
}

fn s(v: &str) -> String {
    v.to_owned()
}

/// `url` parsed against `base` when one is given (not undefined).
fn parse_with_base<E: Engine>(e: &mut E, h: &mut DomHost<E>, url: &str, base: E::Value) -> Result<Option<Url>, Thrown> {
    if base == E::undefined() {
        return Ok(Url::parse(url).ok());
    }
    let base = usv_string(e, h, base)?;
    Ok(Url::parse(&base).ok().and_then(|b| b.join(url).ok()))
}

fn new_url<E: Engine>(h: &mut DomHost<E>, url: Url) -> SideRef {
    h.sides.add(SideData::Url(Box::new(UrlState { url, params: None })))
}

/// Put `new` in place of URL `r`'s URL; its `searchParams` take the new
/// query's pairs.
pub(crate) fn set_url<E: Engine>(h: &mut DomHost<E>, r: SideRef, new: Url) {
    let u = state(h, r);
    let pairs = new.query_pairs().into_owned().collect();
    u.url = new;
    if let Some(Held::Side(p)) = &u.params {
        let p = *p;
        super::url_search_params::replace_list(h, p, pairs);
    }
}

pub(crate) fn constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, url: String, base: E::Value) -> Result<SideRef, Thrown> {
    match parse_with_base(e, h, &url, base)? {
        Some(u) => Ok(new_url(h, u)),
        None => Err(throw(e, ErrorKind::Type, "not a valid URL")),
    }
}

pub(crate) fn parse<E: Engine>(e: &mut E, h: &mut DomHost<E>, url: String, base: E::Value) -> Result<Option<SideRef>, Thrown> {
    Ok(parse_with_base(e, h, &url, base)?.map(|u| new_url(h, u)))
}

pub(crate) fn can_parse<E: Engine>(e: &mut E, h: &mut DomHost<E>, url: String, base: E::Value) -> Result<bool, Thrown> {
    Ok(parse_with_base(e, h, &url, base)?.is_some())
}

pub(crate) fn href<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    Ok(s(url(h, this).as_str()))
}

pub(crate) fn to_string<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    href(e, h, this)
}

pub(crate) fn to_json<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    href(e, h, this)
}

pub(crate) fn set_href<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, v: String) -> Result<(), Thrown> {
    match Url::parse(&v) {
        Ok(u) => {
            set_url(h, this, u);
            Ok(())
        }
        Err(_) => Err(throw(e, ErrorKind::Type, "not a valid URL")),
    }
}

pub(crate) fn origin<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    Ok(url(h, this).origin().ascii_serialization())
}

pub(crate) fn protocol<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    Ok(format!("{}:", url(h, this).scheme()))
}

pub(crate) fn username<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    Ok(s(url(h, this).username()))
}

pub(crate) fn password<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    Ok(s(url(h, this).password().unwrap_or("")))
}

pub(crate) fn host<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    let u = url(h, this);
    Ok(match (u.host_str(), u.port()) {
        (None, _) => String::new(),
        (Some(host), None) => s(host),
        (Some(host), Some(port)) => format!("{host}:{port}"),
    })
}

pub(crate) fn hostname<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    Ok(s(url(h, this).host_str().unwrap_or("")))
}

pub(crate) fn port<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    Ok(url(h, this).port().map_or_else(String::new, |p| p.to_string()))
}

pub(crate) fn pathname<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    Ok(s(url(h, this).path()))
}

/// `part` with `prefix` before it; empty when it is absent or empty.
fn prefixed(prefix: char, part: Option<&str>) -> String {
    match part {
        Some(p) if !p.is_empty() => format!("{prefix}{p}"),
        _ => String::new(),
    }
}

pub(crate) fn search<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    Ok(prefixed('?', url(h, this).query()))
}

pub(crate) fn hash<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    Ok(prefixed('#', url(h, this).fragment()))
}

/// A setter: the URL `f` makes of the current one and `v`, if any.
fn setter<E: Engine>(h: &mut DomHost<E>, this: SideRef, v: &str, f: fn(&Url, &str) -> Option<Url>) -> Result<(), Thrown> {
    if let Some(u) = f(url(h, this), v) {
        set_url(h, this, u);
    }
    Ok(())
}

macro_rules! setters {
    ($($name:ident => $f:path),*) => {$(
        pub(crate) fn $name<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, v: String) -> Result<(), Thrown> {
            setter(h, this, &v, $f)
        }
    )*};
}

setters!(
    set_protocol => parts::protocol,
    set_username => parts::username,
    set_password => parts::password,
    set_host => parts::host,
    set_hostname => parts::hostname,
    set_port => parts::port,
    set_pathname => parts::pathname,
    set_search => parts::search,
    set_hash => parts::hash
);

/// Made when first read; its wrapper is then a member of the URL's
/// (`holds.rs`), so what script puts on it stays while the URL lives.
pub(crate) fn search_params<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<SideRef, Thrown> {
    if let Some(Held::Side(p)) = &state(h, this).params {
        return Ok(*p);
    }
    let pairs = url(h, this).query_pairs().into_owned().collect();
    let p = super::url_search_params::new_params(h, pairs, Some(this));
    let held = h.hold_target(Target::Side(p));
    state(h, this).params = Some(held);
    if let Some(w) = h.live_side_wrapper(e, this) {
        h.keep_target(e, this, w.get());
        e.release(w);
    }
    Ok(p)
}

pub(crate) fn create_object_url<E: Engine>(e: &mut E, h: &mut DomHost<E>, obj: E::Value) -> Result<DomString, Thrown> {
    let Some(blob) = super::blob::blob_of(e, h, obj) else { return Err(throw(e, ErrorKind::Type, "createObjectURL takes a Blob")) };
    let Some(w) = e.current_realm().and_then(|r| h.window_of_realm(r)) else {
        return Err(throw(e, ErrorKind::Type, "the window is gone"));
    };
    match h.blob_urls.add(h.origin_of(w).ascii_serialization(), w, blob) {
        Some(u) => Ok(DomString::from_latin1(u.as_bytes())),
        None => Err(throw_dom(e, h, "QuotaExceededError", "the window has as many blob URLs as it may")),
    }
}

/// Only blob URLs of the calling window's origin are revoked.
pub(crate) fn revoke_object_url<E: Engine>(e: &mut E, h: &mut DomHost<E>, url: DomString) -> Result<(), Thrown> {
    let Some(w) = e.current_realm().and_then(|r| h.window_of_realm(r)) else { return Ok(()) };
    let origin = h.origin_of(w);
    h.blob_urls.revoke(&crate::convert::usv(&url), &origin);
    Ok(())
}
