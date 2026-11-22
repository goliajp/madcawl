//! WTF-8: UTF-8 that may also carry lone surrogates.

pub(crate) fn push_wtf8(out: &mut Vec<u8>, c: u32) {
    match c {
        0..=0x7F => out.push(c as u8),
        0x80..=0x7FF => out.extend_from_slice(&[0xC0 | (c >> 6) as u8, 0x80 | (c & 0x3F) as u8]),
        0x800..=0xFFFF => out.extend_from_slice(&[
            0xE0 | (c >> 12) as u8,
            0x80 | ((c >> 6) & 0x3F) as u8,
            0x80 | (c & 0x3F) as u8,
        ]),
        _ => out.extend_from_slice(&[
            0xF0 | (c >> 18) as u8,
            0x80 | ((c >> 12) & 0x3F) as u8,
            0x80 | ((c >> 6) & 0x3F) as u8,
            0x80 | (c & 0x3F) as u8,
        ]),
    }
}
