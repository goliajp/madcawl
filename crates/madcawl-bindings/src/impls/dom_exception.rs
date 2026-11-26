//! The DOMException interface of Web IDL: what the bindings throw for
//! a DOM error, and what script may make itself.

use madcawl_js::Engine;
use madcawl_str::DomString;

use crate::convert::Thrown;
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

pub(crate) struct DomException {
    pub name: DomString,
    pub message: DomString,
}

/// The legacy `code` of an error name; 0 for a name that has none.
pub(crate) fn legacy_code(name: &DomString) -> u16 {
    let mut b = Vec::new();
    name.write_wtf8(&mut b);
    const NAMES: [&[u8]; 25] = [
        b"IndexSizeError", b"", b"HierarchyRequestError", b"WrongDocumentError", b"InvalidCharacterError", b"",
        b"NoModificationAllowedError", b"NotFoundError", b"NotSupportedError", b"InUseAttributeError",
        b"InvalidStateError", b"SyntaxError", b"InvalidModificationError", b"NamespaceError", b"InvalidAccessError",
        b"", b"TypeMismatchError", b"SecurityError", b"NetworkError", b"AbortError", b"URLMismatchError",
        b"QuotaExceededError", b"TimeoutError", b"InvalidNodeTypeError", b"DataCloneError",
    ];
    NAMES.iter().position(|&n| !n.is_empty() && n == b.as_slice()).map_or(0, |i| i as u16 + 1)
}

pub(crate) fn constructor<E: Engine>(_: &mut E, h: &mut DomHost<E>, message: DomString, name: DomString) -> Result<SideRef, Thrown> {
    Ok(h.sides.add(SideData::DomException(DomException { name, message })))
}

fn this<E: Engine>(h: &DomHost<E>, r: SideRef) -> &DomException {
    match &h.sides.get(r).data {
        SideData::DomException(x) => x,
        _ => unreachable!("a DOMException's side object"),
    }
}

pub(crate) fn name<E: Engine>(_: &mut E, h: &mut DomHost<E>, r: SideRef) -> Result<DomString, Thrown> {
    Ok(this(h, r).name.clone())
}

pub(crate) fn message<E: Engine>(_: &mut E, h: &mut DomHost<E>, r: SideRef) -> Result<DomString, Thrown> {
    Ok(this(h, r).message.clone())
}

pub(crate) fn code<E: Engine>(_: &mut E, h: &mut DomHost<E>, r: SideRef) -> Result<u16, Thrown> {
    Ok(legacy_code(&this(h, r).name))
}
