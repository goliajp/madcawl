//! Where a cookie with a `Domain` attribute may be stored (RFC 6265bis
//! §5.7): the attribute in ASCII form, refused when it names a public
//! suffix other than the host itself, which keeps a host-only cookie.

use crate::psl;
use alt_url::Url;
use std::borrow::Cow;

/// The attribute's domain in ASCII form; a non-ASCII one goes through
/// the URL parser's IDNA, and `None` when that fails or the text could
/// not be a host at all.
fn ascii(attr: &str) -> Option<Cow<'_, str>> {
    if attr.is_ascii() {
        return Some(Cow::Borrowed(attr));
    }
    if attr.chars().any(|c| c.is_whitespace() || "/\\?#@:[]%".contains(c)) {
        return None;
    }
    let url = Url::parse(&format!("http://{attr}/")).ok()?;
    Some(Cow::Owned(url.host_str()?.to_owned()))
}

/// The cookie's domain and whether it is host-only, for a `Domain`
/// attribute `attr` (lowercase, leading dot removed) set from `host`;
/// `None` when the cookie is to be ignored.
pub(super) fn admit(attr: &str, host: &str, ip: bool) -> Option<(String, bool)> {
    let d = ascii(attr)?;
    if d == host && (ip || psl::is_public_suffix(&d)) {
        return Some((host.to_owned(), true));
    }
    if ip || psl::is_public_suffix(&d) || !super::domain_matches(host, &d, false) {
        return None;
    }
    Some((d.into_owned(), false))
}
