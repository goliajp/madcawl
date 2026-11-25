//! The static dictionary (section 8) and its word transforms.
//!
//! `dictionary.bin` is the DICT array of RFC 7932 appendix A as raw
//! bytes, the same file as `c/common/dictionary.bin` in the reference
//! implementation: 122784 bytes, CRC-32 0x5136cb04, SHA-256
//! 20e42eb1b511c21806d4d227d07e5dd06877d8ce7b3a817f378f313653f35c70.

use crate::Error;
use crate::transforms::{Kind, TRANSFORMS};

pub(crate) static DICT: &[u8; 122_784] = include_bytes!("dictionary.bin");

/// log2 of the number of words of each length
const NDBITS: [u8; 25] = [0, 0, 0, 0, 10, 10, 11, 11, 10, 10, 10, 10, 10, 9, 9, 8, 7, 7, 8, 7, 7, 6, 6, 5, 5];

/// Where the words of each length start
const OFFSETS: [usize; 25] = {
    let mut o = [0usize; 25];
    let mut len = 0;
    while len < 24 {
        let words = if NDBITS[len] == 0 { 0 } else { 1 << NDBITS[len] };
        o[len + 1] = o[len] + len * words;
        len += 1;
    }
    o
};

/// The longest transformed word: a 24-byte word and 13 added bytes
pub(crate) const MAX_WORD: usize = 37;

/// Writes the transformed word `id` of length `len` (4..=24) to `out`
/// and returns its length
pub(crate) fn word(len: usize, id: usize, out: &mut [u8; MAX_WORD]) -> Result<usize, Error> {
    let bits = NDBITS[len];
    let index = id & ((1 << bits) - 1);
    let Some(&(prefix, kind, suffix)) = TRANSFORMS.get(id >> bits) else {
        return Err(Error::Format("dictionary transform out of range"));
    };
    let start = OFFSETS[len] + index * len;
    let base = &DICT[start..start + len];
    let body = match kind {
        Kind::OmitFirst(k) => &base[(k as usize).min(len)..],
        Kind::OmitLast(k) => &base[..len.saturating_sub(k as usize)],
        _ => base,
    };
    let mut n = prefix.len();
    out[..n].copy_from_slice(prefix);
    out[n..n + body.len()].copy_from_slice(body);
    let w = &mut out[n..n + body.len()];
    match kind {
        Kind::FermentFirst if !w.is_empty() => {
            ferment(w, 0);
        }
        Kind::FermentAll => {
            let mut i = 0;
            while i < w.len() {
                i += ferment(w, i);
            }
        }
        _ => {}
    }
    n += body.len();
    out[n..n + suffix.len()].copy_from_slice(suffix);
    Ok(n + suffix.len())
}

/// Upper-cases the character at `pos` the way the format defines it
/// and returns its length in bytes
fn ferment(w: &mut [u8], pos: usize) -> usize {
    let b = w[pos];
    if b < 192 {
        if b.is_ascii_lowercase() {
            w[pos] ^= 32;
        }
        1
    } else if b < 224 {
        if pos + 1 < w.len() {
            w[pos + 1] ^= 32;
        }
        2
    } else {
        if pos + 2 < w.len() {
            w[pos + 2] ^= 5;
        }
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_offsets_end_at_the_dictionary_size() {
        assert_eq!(OFFSETS[24] + 24 * (1 << NDBITS[24]), DICT.len());
    }

    #[test]
    fn transforms_apply_prefix_body_and_suffix() {
        let mut out = [0u8; MAX_WORD];
        // the first 4-byte word is "time"
        let n = word(4, 0, &mut out).unwrap();
        assert_eq!(&out[..n], b"time");
        // transform 9: FermentFirst; transform 44: FermentAll
        let n = word(4, 9 << 10, &mut out).unwrap();
        assert_eq!(&out[..n], b"Time");
        let n = word(4, 44 << 10, &mut out).unwrap();
        assert_eq!(&out[..n], b"TIME");
        // transform 49: OmitLast1 then "ing "
        let n = word(4, 49 << 10, &mut out).unwrap();
        assert_eq!(&out[..n], b"timing ");
        assert!(word(4, 121 << 10, &mut out).is_err());
        // the longest: a 24-byte word with the longest prefix and suffix
        for t in 0..121 {
            assert!(word(24, t << 5 | 31, &mut out).unwrap() <= MAX_WORD);
        }
    }
}
