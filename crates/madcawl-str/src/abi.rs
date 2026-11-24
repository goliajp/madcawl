//! The block layout madcawl shares with its script engine, as
//! torajs-abi defines it, and the 8-byte header read as one word.
//! Nothing else in madcawl reads or writes these bits directly.

use std::mem::{offset_of, size_of};

pub use torajs_abi::str::{STR_DATA_OFF, STR_FLAG_IS_ASCII, STR_FLAG_IS_LATIN1, STR_LEN_OFF, STR_MAX_LEN, STR_PAD_OFF};
pub use torajs_abi::{FLAG_STATIC_LITERAL, HEADER_FLAGS_OFF, HEADER_TAG_OFF, HEAP_HEADER_SIZE, HeapHeader, Tag};

// the header word is read and written as a native integer
const _: () = assert!(cfg!(target_endian = "little"));
const _: () = assert!(HEAP_HEADER_SIZE == size_of::<u64>());

pub const REFCOUNT_OFF: usize = offset_of!(HeapHeader, refcount);
/// The tag of a string block. Whether a script value is a string is
/// the engine's to say, never read from a tag.
pub const TAG_STR: u16 = Tag::Str as u16;

const COUNT_SHIFT: u32 = REFCOUNT_OFF as u32 * 8;
const TAG_SHIFT: u32 = HEADER_TAG_OFF as u32 * 8;
const FLAGS_SHIFT: u32 = HEADER_FLAGS_OFF as u32 * 8;

/// The header word with a count of one.
pub const fn header(tag: u16, flags: u16) -> u64 {
    1 << COUNT_SHIFT | (tag as u64) << TAG_SHIFT | (flags as u64) << FLAGS_SHIFT
}

/// The header word of a block that is never counted or freed.
pub const fn static_header(tag: u16, flags: u16) -> u64 {
    (tag as u64) << TAG_SHIFT | ((flags | FLAG_STATIC_LITERAL) as u64) << FLAGS_SHIFT
}

pub const fn refcount(header: u64) -> u32 {
    (header >> COUNT_SHIFT) as u32
}

/// `header` with its count replaced by `count`.
pub const fn with_refcount(header: u64, count: u32) -> u64 {
    header & !(0xFFFF_FFFF << COUNT_SHIFT) | (count as u64) << COUNT_SHIFT
}

pub const fn tag(header: u64) -> u16 {
    (header >> TAG_SHIFT) as u16
}

pub const fn flags(header: u64) -> u16 {
    (header >> FLAGS_SHIFT) as u16
}
