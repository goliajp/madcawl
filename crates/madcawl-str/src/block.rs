//! Reading a string block, whoever owns it.

use std::alloc::Layout;
use std::ptr::NonNull;

use crate::abi;

use crate::wtf8::push_wtf8;

/// A pointer to a string block. Whoever holds one keeps the block
/// alive and unchanged for as long as they read through it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Block(pub(crate) NonNull<u8>);

impl Block {
    pub(crate) fn header(self) -> u64 {
        // SAFETY: every block starts with an initialised header word
        unsafe { self.0.as_ptr().cast::<u64>().read() }
    }

    pub(crate) fn flags(self) -> u16 {
        abi::flags(self.header())
    }

    /// The payload and its storage, borrowed for `'a`.
    ///
    /// # Safety
    /// The block stays alive and unchanged for `'a`.
    pub(crate) unsafe fn view<'a>(self) -> View<'a> {
        let flags = self.flags();
        let latin1 = flags & abi::STR_FLAG_IS_LATIN1 != 0;
        // SAFETY: the length word follows the header
        let len = unsafe { self.0.as_ptr().add(abi::STR_LEN_OFF).cast::<u32>().read() as usize };
        let n = if latin1 { len } else { len * 2 };
        // SAFETY: the payload of `n` bytes follows the prefix, and the
        // caller keeps it alive for 'a
        let payload =
            unsafe { std::slice::from_raw_parts(self.0.as_ptr().add(abi::STR_DATA_OFF), n) };
        View {
            payload,
            latin1,
            ascii: flags & abi::STR_FLAG_IS_ASCII != 0,
        }
    }
}

pub(crate) fn layout(payload: usize) -> Layout {
    Layout::from_size_align(abi::STR_DATA_OFF + payload, 8).expect("string size")
}

/// The content of a block.
#[derive(Clone, Copy)]
pub(crate) struct View<'a> {
    pub(crate) payload: &'a [u8],
    pub(crate) latin1: bool,
    pub(crate) ascii: bool,
}

impl<'a> View<'a> {
    pub(crate) fn len(self) -> usize {
        if self.latin1 {
            self.payload.len()
        } else {
            self.payload.len() / 2
        }
    }

    pub(crate) fn as_latin1(self) -> Option<&'a [u8]> {
        self.latin1.then_some(self.payload)
    }

    pub(crate) fn as_ascii_str(self) -> Option<&'a str> {
        // SAFETY: an ASCII payload is valid UTF-8
        self.ascii
            .then(|| unsafe { std::str::from_utf8_unchecked(self.payload) })
    }

    pub(crate) fn unit(self, i: usize) -> u16 {
        let p = self.payload;
        if self.latin1 {
            u16::from(p[i])
        } else {
            u16::from_ne_bytes([p[2 * i], p[2 * i + 1]])
        }
    }

    pub(crate) fn units(self) -> Units<'a> {
        if self.latin1 {
            Units::Latin1(self.payload.iter())
        } else {
            Units::Utf16(self.payload.as_chunks::<2>().0.iter())
        }
    }

    pub(crate) fn write_wtf8(self, out: &mut Vec<u8>) {
        if let Some(s) = self.as_ascii_str() {
            out.extend_from_slice(s.as_bytes());
            return;
        }
        let mut units = self.units().peekable();
        while let Some(u) = units.next() {
            let mut c = u32::from(u);
            if (0xD800..0xDC00).contains(&u)
                && let Some(&lo) = units.peek()
                && (0xDC00..0xE000).contains(&lo)
            {
                units.next();
                c = 0x1_0000 + ((c - 0xD800) << 10) + (u32::from(lo) - 0xDC00);
            }
            push_wtf8(out, c);
        }
    }

    pub(crate) fn eq_str(self, s: &str) -> bool {
        if let Some(a) = self.as_ascii_str() {
            return a == s;
        }
        let mut theirs = s.encode_utf16();
        self.units().all(|u| theirs.next() == Some(u)) && theirs.next().is_none()
    }

    /// Equal code unit for code unit, whatever the two storages.
    pub(crate) fn eq(self, other: View<'_>) -> bool {
        if self.len() != other.len() {
            return false;
        }
        match (self.as_latin1(), other.as_latin1()) {
            (Some(a), Some(b)) => a == b,
            _ => self.units().eq(other.units()),
        }
    }

    /// Hash by code unit, so the two storages of one text hash alike.
    pub(crate) fn hash<H: std::hash::Hasher>(self, h: &mut H) {
        h.write_usize(self.len());
        for u in self.units() {
            h.write_u16(u);
        }
    }
}

/// The code units of a payload, with the storage decided once.
pub(crate) enum Units<'a> {
    Latin1(std::slice::Iter<'a, u8>),
    Utf16(std::slice::Iter<'a, [u8; 2]>),
}

impl Iterator for Units<'_> {
    type Item = u16;

    fn next(&mut self) -> Option<u16> {
        match self {
            Units::Latin1(it) => it.next().map(|&b| u16::from(b)),
            Units::Utf16(it) => it.next().map(|&p| u16::from_ne_bytes(p)),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Units::Latin1(it) => it.size_hint(),
            Units::Utf16(it) => it.size_hint(),
        }
    }
}

impl ExactSizeIterator for Units<'_> {}
