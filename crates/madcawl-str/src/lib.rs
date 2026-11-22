//! Strings the DOM and the script engine share without copying, and
//! the atoms names are interned as.
//!
//! A string is one heap block in the engine's layout (`madcawl-abi`):
//! a counted header, the length in UTF-16 code units, and the payload
//! stored one byte per code unit when every unit fits in Latin-1, two
//! bytes otherwise. Most text on the web is Latin-1, so most strings
//! cost one byte a character and pass to script as they are.
//!
//! The count is not atomic: a `DomString` stays on the thread that
//! made it, as every engine object does. A `StaticStr` is a block that
//! is never counted and never freed, which any thread may read.

use std::alloc::{alloc, dealloc};
use std::marker::PhantomData;
use std::ptr::NonNull;

use madcawl_abi as abi;

mod atom;
mod block;
mod static_str;
mod wtf8;

pub use atom::{Atom, HeldAtoms, NameHash, STATIC_ATOMS};
pub use static_str::StaticStr;

use block::{Block, View};
use wtf8::Wtf8Chars;

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
        Self::from_block(NonNull::from(&EMPTY).cast::<u8>())
    }

    fn from_block(block: NonNull<u8>) -> Self {
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

    /// A string of Latin-1 code units, one byte each.
    pub fn from_latin1(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self::empty();
        }
        Self::latin1(bytes, bytes.is_ascii())
    }

    /// A string of UTF-16 code units, which need not be well formed.
    /// It is stored as Latin-1 when every unit fits.
    pub fn from_utf16(units: &[u16]) -> Self {
        if units.is_empty() {
            return Self::empty();
        }
        if units.iter().all(|&u| u < 0x100) {
            let ascii = units.iter().all(|&u| u < 0x80);
            let flags = abi::STR_FLAG_IS_LATIN1 | if ascii { abi::STR_FLAG_IS_ASCII } else { 0 };
            let mut out = Self::alloc(units.len(), flags);
            for (d, &u) in out.payload_mut().iter_mut().zip(units) {
                *d = u as u8;
            }
            return out;
        }
        let mut out = Self::alloc(units.len(), 0);
        for (d, &u) in out
            .payload_mut()
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(units)
        {
            *d = u.to_ne_bytes();
        }
        out
    }

    fn latin1(bytes: &[u8], ascii: bool) -> Self {
        let flags = abi::STR_FLAG_IS_LATIN1 | if ascii { abi::STR_FLAG_IS_ASCII } else { 0 };
        let mut out = Self::alloc(bytes.len(), flags);
        out.payload_mut().copy_from_slice(bytes);
        out
    }

    /// A new block with the same content, for a string that is read
    /// on another thread than the one that counts it.
    pub(crate) fn copy(&self) -> Self {
        let v = self.view();
        let mut out = Self::alloc(
            v.len(),
            self.flags() & (abi::STR_FLAG_IS_LATIN1 | abi::STR_FLAG_IS_ASCII),
        );
        out.payload_mut().copy_from_slice(v.payload);
        out
    }

    /// A fresh block for `units` code units; the payload is filled by
    /// the caller.
    fn alloc(units: usize, flags: u16) -> Self {
        assert!(
            units <= abi::STR_MAX_LEN,
            "a string longer than 2^31 - 1 code units"
        );
        let bytes = if flags & abi::STR_FLAG_IS_LATIN1 != 0 {
            units
        } else {
            units * 2
        };
        let layout = block::layout(bytes);
        // SAFETY: the layout has a nonzero size (the 16-byte prefix)
        let p = unsafe { alloc(layout) };
        let Some(block) = NonNull::new(p) else {
            std::alloc::handle_alloc_error(layout)
        };
        // SAFETY: the block is at least 16 bytes and 8-aligned
        unsafe {
            let p = block.as_ptr();
            p.cast::<u64>().write(abi::header(abi::TAG_STR, flags));
            p.add(abi::STR_LEN_OFF).cast::<u32>().write(units as u32);
            p.add(abi::STR_POOL_OFF).cast::<u32>().write(0);
        }
        Self::from_block(block)
    }

    fn header(&self) -> u64 {
        Block(self.block).header()
    }

    fn flags(&self) -> u16 {
        abi::flags(self.header())
    }

    fn view(&self) -> View<'_> {
        // SAFETY: this owner keeps the block alive while it is borrowed,
        // and nobody writes a block that has been handed out
        unsafe { Block(self.block).view() }
    }

    /// Length in UTF-16 code units, as script sees it.
    pub fn len(&self) -> usize {
        self.view().len()
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
        self.view().payload.len()
    }

    fn payload_mut(&mut self) -> &mut [u8] {
        let n = self.payload_len();
        // SAFETY: the payload of `n` bytes follows the prefix; only
        // called on a block just allocated
        unsafe { std::slice::from_raw_parts_mut(self.block.as_ptr().add(abi::STR_DATA_OFF), n) }
    }

    /// The code units, one byte each, when the string is Latin-1.
    pub fn as_latin1(&self) -> Option<&[u8]> {
        self.view().as_latin1()
    }

    /// The text as UTF-8, without copying, when it is ASCII.
    pub fn as_ascii_str(&self) -> Option<&str> {
        self.view().as_ascii_str()
    }

    /// Code unit `i`.
    pub fn unit(&self, i: usize) -> u16 {
        self.view().unit(i)
    }

    pub fn units(&self) -> impl ExactSizeIterator<Item = u16> + '_ {
        self.view().units()
    }

    /// Append the text as WTF-8.
    pub fn write_wtf8(&self, out: &mut Vec<u8>) {
        self.view().write_wtf8(out);
    }

    /// Equal to `s` code unit for code unit.
    pub fn eq_str(&self, s: &str) -> bool {
        self.view().eq_str(s)
    }
}

impl From<StaticStr> for DomString {
    /// The same block: a static one is never counted.
    fn from(s: StaticStr) -> Self {
        Self::from_block(s.block().0)
    }
}

impl Clone for DomString {
    fn clone(&self) -> Self {
        if self.flags() & abi::FLAG_STATIC == 0 {
            // SAFETY: a counted block's first word is its count
            unsafe {
                let rc = self.block.as_ptr().cast::<u32>();
                let n = rc.read();
                // wrapping to zero would free a block that still has
                // owners; a release build does not check the addition
                if n == u32::MAX {
                    std::process::abort();
                }
                rc.write(n + 1);
            }
        }
        Self::from_block(self.block)
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
                dealloc(self.block.as_ptr(), block::layout(self.payload_len()));
            } else {
                rc.write(n);
            }
        }
    }
}

impl PartialEq for DomString {
    fn eq(&self, other: &Self) -> bool {
        self.block == other.block || self.view().eq(other.view())
    }
}

impl Eq for DomString {}

impl std::hash::Hash for DomString {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.view().hash(h);
    }
}

impl std::fmt::Debug for DomString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut v = Vec::new();
        self.write_wtf8(&mut v);
        write!(f, "{:?}", String::from_utf8_lossy(&v))
    }
}

#[cfg(test)]
mod tests;
