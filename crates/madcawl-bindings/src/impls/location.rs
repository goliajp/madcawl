//! The Location interface: the parts of the document's URL, as the HTML
//! Standard serializes them, and navigation by it (`nav.rs`).

use alt_url::Url;
use alt_url::percent::{self, EncodeSet};
use madcawl_js::Engine;
use madcawl_str::DomString;

use crate::convert::{Thrown, throw_dom};
use crate::host::DomHost;
use crate::side::SideRef;

fn s(v: &str) -> DomString {
    DomString::from_wtf8(v.as_bytes())
}

/// `part` with `prefix` before it; empty when it is absent or empty.
fn prefixed(prefix: char, part: Option<&str>) -> DomString {
    match part {
        Some(p) if !p.is_empty() => s(&format!("{prefix}{p}")),
        _ => DomString::empty(),
    }
}

fn text(v: &DomString) -> String {
    let mut b = Vec::new();
    v.write_wtf8(&mut b);
    String::from_utf8_lossy(&b).into_owned()
}

/// `url` against the document's URL, or a "SyntaxError".
fn parse<E: Engine>(e: &mut E, h: &mut DomHost<E>, url: &DomString) -> Result<Url, Thrown> {
    match h.nav.url.join(&text(url)) {
        Ok(u) => Ok(u),
        Err(_) => Err(throw_dom(e, h, "SyntaxError", "not a valid URL")),
    }
}

pub(crate) fn href<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<DomString, Thrown> {
    Ok(s(h.nav.url.as_str()))
}

pub(crate) fn set_href<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef, v: DomString) -> Result<(), Thrown> {
    let u = parse(e, h, &v)?;
    h.location_navigate(e, u, false);
    Ok(())
}

pub(crate) fn to_string<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    href(e, h, this)
}

pub(crate) fn origin<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<DomString, Thrown> {
    Ok(s(&h.nav.url.origin().ascii_serialization()))
}

pub(crate) fn protocol<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<DomString, Thrown> {
    Ok(s(&format!("{}:", h.nav.url.scheme())))
}

pub(crate) fn host<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<DomString, Thrown> {
    let u = &h.nav.url;
    Ok(match (u.host_str(), u.port()) {
        (None, _) => DomString::empty(),
        (Some(host), None) => s(host),
        (Some(host), Some(port)) => s(&format!("{host}:{port}")),
    })
}

pub(crate) fn hostname<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<DomString, Thrown> {
    Ok(h.nav.url.host_str().map_or_else(DomString::empty, s))
}

pub(crate) fn port<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<DomString, Thrown> {
    Ok(h.nav.url.port().map_or_else(DomString::empty, |p| s(&p.to_string())))
}

pub(crate) fn pathname<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<DomString, Thrown> {
    Ok(s(h.nav.url.path()))
}

pub(crate) fn search<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<DomString, Thrown> {
    Ok(prefixed('?', h.nav.url.query()))
}

pub(crate) fn hash<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<DomString, Thrown> {
    Ok(prefixed('#', h.nav.url.fragment()))
}

/// The URL with its fragment set to `v` as the URL Standard's fragment
/// state override sets it (a leading `#` dropped, tabs and newlines
/// removed, spaces kept, percent-encoded), then a fragment navigation
/// when the fragment changed.
pub(crate) fn set_hash<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef, v: DomString) -> Result<(), Thrown> {
    let input = text(&v);
    let input = input.strip_prefix('#').unwrap_or(&input);
    let input: String = input.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    let mut joined = format!("{}#", crate::nav::without_fragment(&h.nav.url));
    percent::encode(&mut joined, &input, EncodeSet::FRAGMENT);
    let Ok(u) = Url::parse(&joined) else { return Err(throw_dom(e, h, "SyntaxError", "not a valid URL")) };
    if u.fragment() != h.nav.url.fragment() {
        h.location_navigate(e, u, false);
    }
    Ok(())
}

pub(crate) fn assign<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef, url: DomString) -> Result<(), Thrown> {
    let u = parse(e, h, &url)?;
    h.location_navigate(e, u, false);
    Ok(())
}

pub(crate) fn replace<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef, url: DomString) -> Result<(), Thrown> {
    let u = parse(e, h, &url)?;
    h.location_navigate(e, u, true);
    Ok(())
}

pub(crate) fn reload<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<(), Thrown> {
    h.record_navigation(crate::nav::Navigation::Reload);
    Ok(())
}
