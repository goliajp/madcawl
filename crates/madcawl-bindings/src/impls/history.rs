//! The History interface over this document's own session history
//! (`nav.rs`): `pushState` and `replaceState` change the URL and state
//! in place; a traversal is recorded and not followed.

use madcawl_js::{Engine, Owned};
use madcawl_str::DomString;

use crate::convert::{Any, Thrown, throw_dom};
use crate::host::DomHost;
use crate::nav::Navigation;
use crate::side::SideRef;

pub(crate) fn length<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<u32, Thrown> {
    Ok(h.nav.length)
}

pub(crate) fn state<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<Any<E>, Thrown> {
    Ok(Any(match &h.nav.state {
        Some(r) => e.retain(r.get()),
        // SAFETY: null carries no count
        None => unsafe { Owned::from_raw(E::null()) },
    }))
}

pub(crate) fn go<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef, delta: i32) -> Result<(), Thrown> {
    h.record_navigation(if delta == 0 { Navigation::Reload } else { Navigation::Traverse(delta) });
    Ok(())
}

pub(crate) fn back<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<(), Thrown> {
    go(e, h, this, -1)
}

pub(crate) fn forward<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<(), Thrown> {
    go(e, h, this, 1)
}

/// The shared history push/replace state steps of the HTML Standard,
/// `data` kept as given.
fn push_or_replace<E: Engine>(e: &mut E, h: &mut DomHost<E>, data: E::Value, url: Option<DomString>, push: bool) -> Result<(), Thrown> {
    let mut new_url = None;
    if let Some(url) = url.filter(|u| !u.is_empty()) {
        let mut b = Vec::new();
        url.write_wtf8(&mut b);
        let parsed = h.nav.url.join(&String::from_utf8_lossy(&b));
        match parsed {
            Ok(u) if crate::nav::can_rewrite(&h.nav.url, &u) => new_url = Some(u),
            Ok(_) => return Err(throw_dom(e, h, "SecurityError", "the URL cannot replace the document's")),
            Err(_) => return Err(throw_dom(e, h, "SecurityError", "not a valid URL")),
        }
    }
    if push {
        h.push_entry(e);
    }
    h.set_state(e, Some(data));
    if let Some(u) = new_url {
        h.nav.url = u;
    }
    Ok(())
}

pub(crate) fn push_state<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef, data: E::Value, _: DomString, url: Option<DomString>) -> Result<(), Thrown> {
    push_or_replace(e, h, data, url, true)
}

pub(crate) fn replace_state<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef, data: E::Value, _: DomString, url: Option<DomString>) -> Result<(), Thrown> {
    push_or_replace(e, h, data, url, false)
}
