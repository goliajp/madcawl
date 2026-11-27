//! `atob` and `btoa`: the forgiving-base64 decode and the base64 encode
//! of the Infra Standard, over strings of code units below 256.

use madcawl_js::{Engine, Owned};
use madcawl_str::DomString;

use crate::convert::{Thrown, arg, dom_string, run, throw_dom};
use crate::host::DomHost;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Each byte's place in the alphabet; 0xFF for one not in it.
const SEXTETS: [u8; 256] = {
    let mut t = [0xFF; 256];
    let mut i = 0;
    while i < 64 {
        t[ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    t
};

fn sextet(c: u16) -> Option<u32> {
    let v = *SEXTETS.get(usize::from(c))?;
    (v != 0xFF).then_some(u32::from(v))
}

/// The forgiving-base64 decode of `units`; `None` on failure.
pub(crate) fn decode(units: impl Iterator<Item = u16>) -> Option<Vec<u8>> {
    let mut data: Vec<u16> = units.filter(|&c| !matches!(c, 0x09 | 0x0A | 0x0C | 0x0D | 0x20)).collect();
    if data.len().is_multiple_of(4) {
        for _ in 0..2 {
            if data.last() == Some(&u16::from(b'=')) {
                data.pop();
            }
        }
    }
    if data.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(data.len() * 3 / 4);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for c in data {
        buffer = (buffer << 6) | sextet(c)?;
        bits += 6;
        if bits == 24 {
            out.extend_from_slice(&[(buffer >> 16) as u8, (buffer >> 8) as u8, buffer as u8]);
            (buffer, bits) = (0, 0);
        }
    }
    match bits {
        12 => out.push((buffer >> 4) as u8),
        18 => out.extend_from_slice(&[(buffer >> 10) as u8, (buffer >> 2) as u8]),
        _ => {}
    }
    Some(out)
}

/// The base64 encode of `bytes`, with padding.
pub(crate) fn encode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            out.push(if i <= chunk.len() { ALPHABET[(n >> (18 - 6 * i)) as usize & 63] } else { b'=' });
        }
    }
    out
}

fn atob<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    run(e, h, |e, h| {
        crate::convert::need(e, args, 1)?;
        let s = dom_string(e, h, arg::<E>(args, 0))?;
        match decode(s.units()) {
            Some(bytes) => Ok(DomString::from_latin1(&bytes)),
            None => Err(invalid(e, h, "the string is not valid base64")),
        }
    })
}

fn btoa<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    run(e, h, |e, h| {
        crate::convert::need(e, args, 1)?;
        let s = dom_string(e, h, arg::<E>(args, 0))?;
        let bytes: Option<Vec<u8>> = s.units().map(|c| u8::try_from(c).ok()).collect();
        match bytes {
            Some(b) => Ok(DomString::from_latin1(&encode(&b))),
            None => Err(invalid(e, h, "the string has a character outside Latin-1")),
        }
    })
}

fn invalid<E: Engine>(e: &mut E, h: &mut DomHost<E>, message: &str) -> Thrown {
    throw_dom(e, h, "InvalidCharacterError", message)
}

// SAFETY: called as a host function made for E and DomHost<E>
pub(crate) unsafe extern "C" fn atob_abi<E: Engine>(ctx: *mut madcawl_js::Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
    unsafe { crate::shell::op(ctx, this, argv, argc, atob::<E>) }
}

// SAFETY: called as a host function made for E and DomHost<E>
pub(crate) unsafe extern "C" fn btoa_abi<E: Engine>(ctx: *mut madcawl_js::Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
    unsafe { crate::shell::op(ctx, this, argv, argc, btoa::<E>) }
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    fn d(s: &str) -> Option<Vec<u8>> {
        decode(s.encode_utf16())
    }

    #[test]
    fn the_forgiving_decode_drops_whitespace_and_up_to_two_trailing_equals() {
        assert_eq!(d("aGVsbG8="), Some(b"hello".to_vec()));
        assert_eq!(d(" aGVs\nbG8 "), Some(b"hello".to_vec()));
        assert_eq!(d("aGVsbG8"), Some(b"hello".to_vec()));
        assert_eq!(d("YQ=="), Some(b"a".to_vec()));
        assert_eq!(d("YQ"), Some(b"a".to_vec()));
        assert_eq!(d(""), Some(Vec::new()));
    }

    #[test]
    fn the_forgiving_decode_fails_on_bad_lengths_and_characters() {
        assert_eq!(d("YQ="), None, "a length of 3 with padding keeps its =");
        assert_eq!(d("Y"), None);
        assert_eq!(d("YQ==="), None);
        assert_eq!(d("Y-Q="), None);
        assert_eq!(d("YQ=a"), None);
        assert_eq!(d("\u{100}AAA"), None);
    }

    #[test]
    fn the_encode_pads_to_four() {
        assert_eq!(encode(b""), b"");
        assert_eq!(encode(b"a"), b"YQ==");
        assert_eq!(encode(b"ab"), b"YWI=");
        assert_eq!(encode(b"abc"), b"YWJj");
        assert_eq!(encode(&[0xff, 0xfe]), b"//4=");
    }
}
