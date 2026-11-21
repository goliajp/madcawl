//! Byte layout of the heap blocks madcawl shares with its script
//! engine: the 8-byte header every block starts with, and the string
//! block. The values follow the engine's own layout so that a block
//! made on either side can be handed to the other as it is. Nothing
//! else in madcawl reads or writes these bits directly.

#![no_std]

/// Every block starts with this: a 32-bit reference count, a 16-bit
/// type tag and 16 bits of flags.
pub const HEADER_SIZE: usize = 8;
pub const REFCOUNT_OFF: usize = 0;
pub const TAG_OFF: usize = 4;
pub const FLAGS_OFF: usize = 6;

/// Type tags.
pub const TAG_STR: u16 = 0;

/// A block that lives for the whole program: never counted, never
/// freed.
pub const FLAG_STATIC: u16 = 1 << 2;

/// String block: header, then the length in code units, then 4
/// reserved bytes, then the payload.
pub const STR_LEN_OFF: usize = 8;
pub const STR_DATA_OFF: usize = 16;
/// The payload is Latin-1, one byte per code unit; otherwise UTF-16.
pub const STR_FLAG_IS_LATIN1: u16 = 0x0002;
/// Every code unit is ASCII, so the Latin-1 payload is also its UTF-8.
pub const STR_FLAG_IS_ASCII: u16 = 1 << 12;

/// The header word with a count of one.
pub const fn header(tag: u16, flags: u16) -> u64 {
    1 | (tag as u64) << 32 | (flags as u64) << 48
}

/// The header word of a static block.
pub const fn static_header(tag: u16, flags: u16) -> u64 {
    (tag as u64) << 32 | ((flags | FLAG_STATIC) as u64) << 48
}

pub const fn refcount(header: u64) -> u32 {
    header as u32
}

pub const fn tag(header: u64) -> u16 {
    (header >> 32) as u16
}

pub const fn flags(header: u64) -> u16 {
    (header >> 48) as u16
}
