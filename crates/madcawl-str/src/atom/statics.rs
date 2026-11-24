//! The string blocks of the static atoms, laid out at compile time in
//! read-only data.

use std::ptr::NonNull;

use crate::abi;

use super::generated::{COUNT, NAMES};
use crate::block::Block;
use crate::static_str::StaticStr;

const fn block_size(len: usize) -> usize {
    (abi::STR_DATA_OFF + len + 7) & !7
}

const OFFSETS: [u32; COUNT] = {
    let mut out = [0; COUNT];
    let mut at = 0;
    let mut i = 0;
    while i < COUNT {
        out[i] = at as u32;
        at += block_size(NAMES[i].len());
        i += 1;
    }
    out
};

const TOTAL: usize = OFFSETS[COUNT - 1] as usize + block_size(NAMES[COUNT - 1].len());

#[repr(C, align(8))]
struct Blocks([u8; TOTAL]);

static BLOCKS: Blocks = Blocks(build());

const fn put(out: &mut [u8; TOTAL], at: usize, bytes: &[u8]) {
    let mut k = 0;
    while k < bytes.len() {
        out[at + k] = bytes[k];
        k += 1;
    }
}

const fn build() -> [u8; TOTAL] {
    // the generator only takes ASCII names
    let header = abi::static_header(
        abi::TAG_STR,
        abi::STR_FLAG_IS_LATIN1 | abi::STR_FLAG_IS_ASCII,
    );
    let mut out = [0; TOTAL];
    let mut i = 0;
    while i < COUNT {
        let at = OFFSETS[i] as usize;
        let name = NAMES[i].as_bytes();
        put(&mut out, at, &header.to_ne_bytes());
        put(
            &mut out,
            at + abi::STR_LEN_OFF,
            &(name.len() as u32).to_ne_bytes(),
        );
        put(&mut out, at + abi::STR_DATA_OFF, name);
        i += 1;
    }
    out
}

/// The block of static atom `i`.
pub(crate) fn get(i: usize) -> StaticStr {
    let p = BLOCKS.0[OFFSETS[i] as usize..].as_ptr();
    // SAFETY: an offset into a static is not null; the block there is
    // a static string block in read-only data, never written
    unsafe { StaticStr::new(Block(NonNull::new_unchecked(p.cast_mut()))) }
}
