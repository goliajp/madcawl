//! The Encoding Standard's TextEncoder: UTF-8 of a USVString, into a new
//! Uint8Array or into one script gives. encodeInto's result is a plain
//! object whose `read` and `written` are set as script would set them.

use madcawl_js::{ArrayType, Engine, ErrorKind, Key};
use madcawl_str::DomString;

use crate::convert::{Any, Thrown, throw};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

pub(crate) fn constructor<E: Engine>(_: &mut E, h: &mut DomHost<E>) -> Result<SideRef, Thrown> {
    Ok(h.sides.add(SideData::TextEncoder))
}

pub(crate) fn encoding<E: Engine>(_: &mut E, _: &mut DomHost<E>, _: SideRef) -> Result<DomString, Thrown> {
    Ok(DomString::from_latin1(b"utf-8"))
}

pub(crate) fn encode<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef, input: String) -> Result<Any<E>, Thrown> {
    let realm = h.caller_realm(e)?;
    let a = e.new_bytes(realm, input.into_bytes(), false);
    if E::is_thrown(a.get()) {
        let _sentinel = a.into_raw();
        return Err(Thrown);
    }
    Ok(Any(a))
}

/// The UTF-16 code units of `source` read and the bytes written: whole
/// scalar values, as many as fit in `dest`.
pub(crate) fn encode_prefix(source: &str, dest: &mut [u8]) -> (usize, usize) {
    let (mut read, mut written) = (0, 0);
    for c in source.chars() {
        let n = c.len_utf8();
        if written + n > dest.len() {
            break;
        }
        c.encode_utf8(&mut dest[written..written + n]);
        read += c.len_utf16();
        written += n;
    }
    (read, written)
}

pub(crate) fn encode_into<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef, source: String, dest: E::Value) -> Result<Any<E>, Thrown> {
    let (read, written) = match e.typed_array(dest) {
        Some((ArrayType::Uint8, b)) => encode_prefix(&source, b),
        _ => return Err(throw(e, ErrorKind::Type, "encodeInto takes a Uint8Array")),
    };
    let realm = h.caller_realm(e)?;
    let o = e.new_object(realm);
    for (k, n) in [("read", read), ("written", written)] {
        if !e.set(h, o.get(), Key::Str(&DomString::from_latin1(k.as_bytes())), E::number(n as f64)) {
            e.release(o);
            return Err(Thrown);
        }
    }
    Ok(Any(o))
}

#[cfg(test)]
mod tests {
    #[test]
    fn encode_into_writes_whole_scalar_values_and_counts_code_units() {
        let mut b = [0u8; 5];
        assert_eq!(super::encode_prefix("a\u{e9}\u{1f600}", &mut b), (2, 3));
        assert_eq!(&b[..3], "a\u{e9}".as_bytes());
        let mut b = [0u8; 7];
        assert_eq!(super::encode_prefix("a\u{e9}\u{1f600}", &mut b), (4, 7));
        assert_eq!(super::encode_prefix("", &mut []), (0, 0));
    }
}
