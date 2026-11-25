//! Where a followed redirect goes, and what of the request goes with it.

use crate::error::NetError;
use alt_url::Url;

/// The redirect target, keeping the fragment when the target has none.
pub(super) fn redirect_url(url: &Url, location: &str) -> Result<Url, NetError> {
    let next = url.join(location).map_err(|e| NetError::BadUrl(format!("{location}: {e:?}")))?;
    match (next.fragment(), url.fragment()) {
        (None, Some(f)) => Ok(next.join(&format!("#{f}")).unwrap_or(next)),
        _ => Ok(next),
    }
}

/// 301 and 302 turn POST into GET, 303 turns anything but GET and HEAD
/// into GET; the body and its headers go with the old method.
pub(super) fn rewrite_for_redirect(status: u16, method: &mut String, body: &mut Option<&[u8]>, headers: &mut Vec<(String, Vec<u8>)>) {
    let rewrite = (matches!(status, 301 | 302) && method == "POST") || (status == 303 && !matches!(method.as_str(), "GET" | "HEAD"));
    if rewrite {
        *method = "GET".into();
        *body = None;
        headers.retain(|(n, _)| {
            let n = n.to_ascii_lowercase();
            !matches!(n.as_str(), "content-encoding" | "content-language" | "content-location" | "content-type")
        });
    }
}
