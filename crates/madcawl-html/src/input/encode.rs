//! Code points written back out as WTF-8.

use super::visible;

/// A code point as WTF-8.
#[inline]
pub(crate) fn encode_char(c: u32) -> ([u8; 4], usize) {
    let c = visible(c);
    match c {
        0..=0x7F => ([c as u8, 0, 0, 0], 1),
        0x80..=0x7FF => ([0xC0 | (c >> 6) as u8, 0x80 | (c & 0x3F) as u8, 0, 0], 2),
        0x800..=0xFFFF => (
            [
                0xE0 | (c >> 12) as u8,
                0x80 | ((c >> 6) & 0x3F) as u8,
                0x80 | (c & 0x3F) as u8,
                0,
            ],
            3,
        ),
        _ => (
            [
                0xF0 | (c >> 18) as u8,
                0x80 | ((c >> 12) & 0x3F) as u8,
                0x80 | ((c >> 6) & 0x3F) as u8,
                0x80 | (c & 0x3F) as u8,
            ],
            4,
        ),
    }
}

/// Append a code point as WTF-8.
#[inline]
pub(crate) fn push_char(out: &mut Vec<u8>, c: u32) {
    if c < 0x80 {
        out.push(c as u8);
    } else {
        let (b, n) = encode_char(c);
        out.extend_from_slice(&b[..n]);
    }
}
