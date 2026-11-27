//! Reflected content attributes (HTML 2.6.1): the IDL attribute reads
//! and writes the element's attribute of the same name.

use madcawl_dom::NodeId;
use madcawl_js::Engine;
use madcawl_str::{Atom, DomString};

use crate::host::DomHost;

/// A `DOMString` attribute: the value, or the empty string.
pub(crate) fn string<E: Engine>(h: &DomHost<E>, el: NodeId, name: Atom) -> DomString {
    h.dom.attribute(el, name).cloned().unwrap_or_else(DomString::empty)
}

/// A `USVString` attribute holding a URL: the empty string without the
/// attribute, the URL parsed against the base URL of the element's node
/// document when it parses, and the value as it is when it does not.
pub(crate) fn url<E: Engine>(h: &DomHost<E>, el: NodeId, name: Atom) -> DomString {
    let Some(v) = h.dom.attribute(el, name) else {
        return DomString::empty();
    };
    let mut b = Vec::new();
    v.write_wtf8(&mut b);
    match h.base_url(h.dom.owner_document(el)).join(&String::from_utf8_lossy(&b)) {
        Ok(u) => DomString::from_wtf8(u.as_str().as_bytes()),
        Err(_) => v.clone(),
    }
}

/// An enumerated attribute with no missing or invalid value default:
/// the keyword of `keywords` (lower case) the value matches ignoring
/// ASCII case, or the empty string.
pub(crate) fn keyword<E: Engine>(h: &DomHost<E>, el: NodeId, name: Atom, keywords: &[&'static str]) -> DomString {
    let Some(v) = h.dom.attribute(el, name) else {
        return DomString::empty();
    };
    let mut b = Vec::new();
    v.write_wtf8(&mut b);
    match keywords.iter().find(|k| k.as_bytes().eq_ignore_ascii_case(&b)) {
        Some(k) => DomString::from_wtf8(k.as_bytes()),
        None => DomString::empty(),
    }
}
