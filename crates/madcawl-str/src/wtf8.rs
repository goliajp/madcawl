//! WTF-8: UTF-8 that may also carry lone surrogates.

/// Code points of WTF-8 bytes. The input comes from the tokenizer and
/// is well formed.
pub(crate) struct Wtf8Chars<'a>(pub(crate) &'a [u8]);

impl Iterator for Wtf8Chars<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        let s = self.0;
        let &b = s.first()?;
        let (n, init) = match b {
            0..=0x7F => (1, u32::from(b)),
            0xC0..=0xDF => (2, u32::from(b & 0x1F)),
            0xE0..=0xEF => (3, u32::from(b & 0x0F)),
            _ => (4, u32::from(b & 0x07)),
        };
        let c = s[1..n]
            .iter()
            .fold(init, |c, &t| (c << 6) | u32::from(t & 0x3F));
        self.0 = &s[n..];
        Some(c)
    }
}

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
