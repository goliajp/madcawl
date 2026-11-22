//! Byte layout of the heap blocks madcawl shares with its script
//! engine: the 8-byte header every block starts with, and the string
//! block. The values follow the engine's own layout so that a block
//! made on either side can be handed to the other as it is. Nothing
//! else in madcawl reads or writes these bits directly.

#![no_std]

// the header word and the length are read and written as native
// integers, and the layout is the little-endian one
const _: () = assert!(cfg!(target_endian = "little"));

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

/// String block: header, then the length in code units (`u32`), then
/// the `u32` at `STR_POOL_OFF`, then the payload.
pub const STR_LEN_OFF: usize = 8;
/// The engine defines this word: the pool class the block was taken
/// from, or its real payload capacity when the engine sets its own
/// capacity flag. madcawl writes 0 there, which means the block came
/// from no pool class (or is static) and its payload is exactly its
/// length.
pub const STR_POOL_OFF: usize = 12;
pub const STR_DATA_OFF: usize = 16;
/// The most code units a string block holds, as in the engine.
pub const STR_MAX_LEN: usize = (1 << 31) - 1;
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
