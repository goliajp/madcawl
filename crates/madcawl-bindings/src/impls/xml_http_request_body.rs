//! The body `send` takes, as the Fetch Standard extracts one from an
//! XMLHttpRequestBodyInit: a Blob's bytes and type, a buffer source's
//! bytes, URLSearchParams serialized as a form, and anything else
//! converted to a USVString and sent as UTF-8 text. A Document or
//! FormData body is not bound yet (neither interface is): it is sent
//! as its string.

use madcawl_js::Engine;

use crate::convert::{Thrown, usv_string};
use crate::host::DomHost;
use crate::side::SideData;

/// The body's bytes and the `Content-Type` it implies, for `body`; no
/// bytes for null and undefined.
pub(crate) fn extract<E: Engine>(e: &mut E, h: &mut DomHost<E>, body: E::Value) -> Result<(Option<Vec<u8>>, Option<String>), Thrown> {
    if body == E::null() || body == E::undefined() {
        return Ok((None, None));
    }
    if let Some(b) = super::blob::blob_of(e, h, body) {
        return Ok((Some(b.bytes().to_vec()), (!b.ty.is_empty()).then_some(b.ty)));
    }
    let realm = h.caller_realm(e)?;
    if let Some(b) = e.buffer_source(realm, body) {
        return Ok((Some(b.to_vec()), None));
    }
    let params = e.host_of(body, &crate::generated::URLSearchParamsClass::<E>::DEF).and_then(|o| h.sides.of(o));
    if let Some(SideData::SearchParams(p)) = params.map(|r| &h.sides.get(r).data) {
        let s = super::url_search_params::serialize(&p.list);
        return Ok((Some(s.into_bytes()), Some("application/x-www-form-urlencoded;charset=UTF-8".to_owned())));
    }
    let s = usv_string(e, h, body)?;
    Ok((Some(s.into_bytes()), Some("text/plain;charset=UTF-8".to_owned())))
}
