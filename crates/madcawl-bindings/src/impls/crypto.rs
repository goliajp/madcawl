//! The Crypto interface: random values from the operating system.

use madcawl_js::{ArrayType, Engine, ErrorKind};
use madcawl_str::DomString;

use crate::convert::{Any, Thrown, throw, throw_dom};
use crate::host::DomHost;
use crate::side::SideRef;

/// The most bytes one `getRandomValues` fills.
const MAX_BYTES: usize = 65536;

/// Fill an integer typed array with random bytes and return it. A
/// DataView is refused with a TypeError, not the standard's
/// "TypeMismatchError", as the engine does not tell it from other
/// objects.
pub(crate) fn get_random_values<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef, array: E::Value) -> Result<Any<E>, Thrown> {
    let refused = match e.typed_array(array) {
        None => return Err(throw(e, ErrorKind::Type, "getRandomValues takes an ArrayBufferView")),
        Some((ArrayType::Other, _)) => Some(("TypeMismatchError", "the array is not of an integer type")),
        Some((_, b)) if b.len() > MAX_BYTES => Some(("QuotaExceededError", "the array is longer than 65536 bytes")),
        Some((_, b)) => {
            crate::random::fill(b);
            None
        }
    };
    match refused {
        Some((name, message)) => Err(throw_dom(e, h, name, message)),
        None => Ok(Any(e.retain(array))),
    }
}

pub(crate) fn random_uuid<E: Engine>(_: &mut E, _: &mut DomHost<E>, _: SideRef) -> Result<DomString, Thrown> {
    let mut b = [0u8; 16];
    crate::random::fill(&mut b);
    Ok(DomString::from_latin1(uuid(b).as_bytes()))
}

/// A version 4 UUID of 16 random bytes, in its lowercase form.
pub(crate) fn uuid(mut b: [u8; 16]) -> String {
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(36);
    for (i, x) in b.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push(char::from(HEX[usize::from(x >> 4)]));
        out.push(char::from(HEX[usize::from(x & 15)]));
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_uuid_has_its_version_and_variant_bits_and_dashes() {
        let u = super::uuid([0xff; 16]);
        assert_eq!(u, "ffffffff-ffff-4fff-bfff-ffffffffffff");
        assert_eq!(super::uuid([0; 16]), "00000000-0000-4000-8000-000000000000");
    }
}
