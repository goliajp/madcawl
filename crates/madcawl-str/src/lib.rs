//! Strings the DOM and the script engine share without copying.
//!
//! A string is one heap block in the engine's layout (`madcawl-abi`):
//! a counted header, the length in UTF-16 code units, and the payload
//! stored one byte per code unit when every unit fits in Latin-1, two
//! bytes otherwise. Most text on the web is Latin-1, so most strings
//! cost one byte a character and pass to script as they are.
//!
//! The count is not atomic: a `DomString` stays on the thread that
//! made it, as every engine object does.

use std::alloc::{Layout, alloc, dealloc};
use std::marker::PhantomData;
use std::ptr::NonNull;

use madcawl_abi as abi;

pub struct DomString {
    block: NonNull<u8>,
    /// Neither `Send` nor `Sync`: the count is not atomic.
    _local: PhantomData<*const u8>,
}

/// The empty string, shared and never freed.
#[repr(C, align(8))]
struct StaticEmpty([u64; 2]);
static EMPTY: StaticEmpty = StaticEmpty([
    abi::static_header(
        abi::TAG_STR,
        abi::STR_FLAG_IS_LATIN1 | abi::STR_FLAG_IS_ASCII,
    ),
    0,
]);

impl DomString {
    pub fn empty() -> Self {
        let block = NonNull::from(&EMPTY).cast::<u8>();
        Self {
            block,
            _local: PhantomData,
        }
    }

    /// A string from WTF-8, as the HTML tokenizer produces it. Lone
    /// surrogates are kept as the code units they stand for.
    pub fn from_wtf8(s: &[u8]) -> Self {
        if s.is_empty() {
            return Self::empty();
        }
        if s.is_ascii() {
            return Self::latin1(s, true);
        }
        let mut units = 0usize;
        let mut latin1 = true;
        for c in Wtf8Chars(s) {
            units += if c > 0xFFFF { 2 } else { 1 };
            latin1 &= c < 0x100;
        }
        if latin1 {
            let mut out = Self::alloc(units, abi::STR_FLAG_IS_LATIN1);
            let dst = out.payload_mut();
            for (d, c) in dst.iter_mut().zip(Wtf8Chars(s)) {
                *d = c as u8;
            }
            return out;
        }
        let mut out = Self::alloc(units, 0);
        let dst = out.payload_mut();
        let mut i = 0;
        for c in Wtf8Chars(s) {
            let mut put = |u: u16| {
                dst[i..i + 2].copy_from_slice(&u.to_ne_bytes());
                i += 2;
            };
            if c > 0xFFFF {
                let c = c - 0x1_0000;
                put(0xD800 | (c >> 10) as u16);
                put(0xDC00 | (c & 0x3FF) as u16);
            } else {
                put(c as u16);
            }
        }
        out
    }

    fn latin1(bytes: &[u8], ascii: bool) -> Self {
        let flags = abi::STR_FLAG_IS_LATIN1 | if ascii { abi::STR_FLAG_IS_ASCII } else { 0 };
        let mut out = Self::alloc(bytes.len(), flags);
        out.payload_mut().copy_from_slice(bytes);
        out
    }

    /// A fresh block for `units` code units; the payload is filled by
    /// the caller.
    fn alloc(units: usize, flags: u16) -> Self {
        let units32 = u32::try_from(units).expect("a string longer than 4G code units");
        let bytes = if flags & abi::STR_FLAG_IS_LATIN1 != 0 {
            units
        } else {
            units * 2
        };
        let layout = Self::layout(bytes);
        // SAFETY: the layout has a nonzero size (the 16-byte prefix)
        let p = unsafe { alloc(layout) };
        let Some(block) = NonNull::new(p) else {
            std::alloc::handle_alloc_error(layout)
        };
        // SAFETY: the block is at least 16 bytes and 8-aligned
        unsafe {
            block
                .as_ptr()
                .cast::<u64>()
                .write(abi::header(abi::TAG_STR, flags));
            block
                .as_ptr()
                .add(abi::STR_LEN_OFF)
                .cast::<u64>()
                .write(u64::from(units32));
        }
        Self {
            block,
            _local: PhantomData,
        }
    }

    fn layout(payload: usize) -> Layout {
        Layout::from_size_align(abi::STR_DATA_OFF + payload, 8).expect("string size")
    }

    fn header(&self) -> u64 {
        // SAFETY: every block starts with an initialised header word
        unsafe { self.block.as_ptr().cast::<u64>().read() }
    }

    fn flags(&self) -> u16 {
        abi::flags(self.header())
    }

    /// Length in UTF-16 code units, as script sees it.
    pub fn len(&self) -> usize {
        // SAFETY: the length word follows the header
        unsafe {
            self.block
                .as_ptr()
                .add(abi::STR_LEN_OFF)
                .cast::<u32>()
                .read() as usize
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_latin1(&self) -> bool {
        self.flags() & abi::STR_FLAG_IS_LATIN1 != 0
    }

    pub fn is_ascii(&self) -> bool {
        self.flags() & abi::STR_FLAG_IS_ASCII != 0
    }

    fn payload_len(&self) -> usize {
        if self.is_latin1() {
            self.len()
        } else {
            self.len() * 2
        }
    }

    fn payload(&self) -> &[u8] {
        // SAFETY: the payload of `payload_len` bytes follows the prefix
        unsafe {
            std::slice::from_raw_parts(
                self.block.as_ptr().add(abi::STR_DATA_OFF),
                self.payload_len(),
            )
        }
    }

    fn payload_mut(&mut self) -> &mut [u8] {
        let n = self.payload_len();
        // SAFETY: as `payload`; only called on a block just allocated
        unsafe { std::slice::from_raw_parts_mut(self.block.as_ptr().add(abi::STR_DATA_OFF), n) }
    }

    /// The code units, one byte each, when the string is Latin-1.
    pub fn as_latin1(&self) -> Option<&[u8]> {
        self.is_latin1().then(|| self.payload())
    }

    /// The text as UTF-8, without copying, when it is ASCII.
    pub fn as_ascii_str(&self) -> Option<&str> {
        // SAFETY: an ASCII payload is valid UTF-8
        self.is_ascii()
            .then(|| unsafe { std::str::from_utf8_unchecked(self.payload()) })
    }

    /// Code unit `i`.
    pub fn unit(&self, i: usize) -> u16 {
        let p = self.payload();
        if self.is_latin1() {
            u16::from(p[i])
        } else {
            u16::from_ne_bytes([p[2 * i], p[2 * i + 1]])
        }
    }

    pub fn units(&self) -> impl Iterator<Item = u16> + '_ {
        (0..self.len()).map(|i| self.unit(i))
    }

    /// Append the text as WTF-8.
    pub fn write_wtf8(&self, out: &mut Vec<u8>) {
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

    /// Equal to `s` code unit for code unit.
    pub fn eq_str(&self, s: &str) -> bool {
        if let Some(a) = self.as_ascii_str() {
            return a == s;
        }
        let mut theirs = s.encode_utf16();
        self.units().all(|u| theirs.next() == Some(u)) && theirs.next().is_none()
    }
}

impl Clone for DomString {
    fn clone(&self) -> Self {
        if self.flags() & abi::FLAG_STATIC == 0 {
            // SAFETY: a counted block's first word is its count
            unsafe {
                let rc = self.block.as_ptr().cast::<u32>();
                rc.write(rc.read() + 1);
            }
        }
        Self {
            block: self.block,
            _local: PhantomData,
        }
    }
}

impl Drop for DomString {
    fn drop(&mut self) {
        if self.flags() & abi::FLAG_STATIC != 0 {
            return;
        }
        // SAFETY: a counted block's first word is its count; the last
        // owner frees it with the layout it was made with
        unsafe {
            let rc = self.block.as_ptr().cast::<u32>();
            let n = rc.read() - 1;
            if n == 0 {
                dealloc(self.block.as_ptr(), Self::layout(self.payload_len()));
            } else {
                rc.write(n);
            }
        }
    }
}

impl PartialEq for DomString {
    fn eq(&self, other: &Self) -> bool {
        if self.block == other.block {
            return true;
        }
        if self.len() != other.len() {
            return false;
        }
        match (self.as_latin1(), other.as_latin1()) {
            (Some(a), Some(b)) => a == b,
            _ => self.units().eq(other.units()),
        }
    }
}

impl Eq for DomString {}

impl std::hash::Hash for DomString {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        // by code unit, so the two storages of one text hash alike
        h.write_usize(self.len());
        for u in self.units() {
            h.write_u16(u);
        }
    }
}

impl std::fmt::Debug for DomString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut v = Vec::new();
        self.write_wtf8(&mut v);
        write!(f, "{:?}", String::from_utf8_lossy(&v))
    }
}

/// Code points of WTF-8 bytes. The input comes from the tokenizer and
/// is well formed.
struct Wtf8Chars<'a>(&'a [u8]);

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

fn push_wtf8(out: &mut Vec<u8>, c: u32) {
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

#[cfg(test)]
mod tests;
