//! The header at the start of a shared-memory surface buffer, written by
//! the display end that makes the buffer and checked by both sides
//! before either touches a pixel. All fields are little-endian:
//!
//! | offset | size | field |
//! |---|---|---|
//! | 0 | 4 | magic `MCWS` |
//! | 4 | 2 | version the writer speaks best |
//! | 6 | 2 | oldest version it still speaks |
//! | 8 | 4 | header length, 64 |
//! | 12 | 4 | pixel format |
//! | 16 | 4 | width, device pixels |
//! | 20 | 4 | height |
//! | 24 | 4 | bytes per row, a multiple of 16 |
//! | 32 | 8 | the buffer's size in bytes, header included |
//! | 40 | 8 | generation: the commit sequence the pixels were last written for |
//!
//! Pixels start at [`HEADER_LEN`], which keeps them 16-byte aligned, as
//! a texture made over the memory needs.

use crate::MAX_SIDE;
use crate::version::{Outcome, Range};

pub const MAGIC: [u8; 4] = *b"MCWS";
pub const HEADER_LEN: usize = 64;
/// The byte offset of the generation, an 8-byte aligned u64.
pub const GENERATION_AT: usize = 40;
/// The surface header versions this build speaks.
pub const RANGE: Range = Range::new(1, 1);
/// The largest buffer, header included.
pub const MAX_BYTES: u64 = 1 << 30;

/// Premultiplied BGRA, 8 bits a channel, sRGB.
pub const BGRA8_SRGB_PREMUL: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub version: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderError {
    Short,
    Magic,
    Version(Outcome),
    Length,
    Format,
    Size,
    Stride,
    /// The size it states is not the memory there is
    Mapped,
}

impl std::fmt::Display for HeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a bad surface header: {self:?}")
    }
}

impl std::error::Error for HeaderError {}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("four bytes"))
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().expect("eight bytes"))
}

/// Whether a surface of `width` by `height` with rows of `stride` bytes
/// fits in `size` bytes, header included, all of it within the bounds.
pub fn fits(width: u32, height: u32, stride: u32, size: u64) -> Result<(), HeaderError> {
    if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
        return Err(HeaderError::Size);
    }
    if !stride.is_multiple_of(16) || u64::from(stride) < u64::from(width) * 4 {
        return Err(HeaderError::Stride);
    }
    let need = HEADER_LEN as u64 + u64::from(stride) * u64::from(height);
    match size >= need && size <= MAX_BYTES {
        true => Ok(()),
        false => Err(HeaderError::Mapped),
    }
}

/// The bytes per row for `width` device pixels: four a pixel, rounded up
/// to 16.
pub fn stride_for(width: u32) -> u32 {
    (width * 4).div_ceil(16) * 16
}

impl Header {
    /// The header for a buffer of `size` bytes, written by this build.
    pub fn new(width: u32, height: u32, size: u64) -> Header {
        let stride = stride_for(width);
        Header { version: RANGE.cur, format: BGRA8_SRGB_PREMUL, width, height, stride, size }
    }

    /// The bytes a buffer of this header needs, header included.
    pub fn needed(width: u32, height: u32) -> u64 {
        HEADER_LEN as u64 + u64::from(stride_for(width)) * u64::from(height)
    }

    pub fn write(&self, b: &mut [u8]) {
        let b = &mut b[..HEADER_LEN];
        b.fill(0);
        b[0..4].copy_from_slice(&MAGIC);
        b[4..6].copy_from_slice(&(self.version as u16).to_le_bytes());
        b[6..8].copy_from_slice(&(RANGE.min as u16).to_le_bytes());
        b[8..12].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
        b[12..16].copy_from_slice(&self.format.to_le_bytes());
        b[16..20].copy_from_slice(&self.width.to_le_bytes());
        b[20..24].copy_from_slice(&self.height.to_le_bytes());
        b[24..28].copy_from_slice(&self.stride.to_le_bytes());
        b[32..40].copy_from_slice(&self.size.to_le_bytes());
    }

    /// The header at the start of `b`, a buffer `mapped` bytes long,
    /// checked against this build's range and the memory there is.
    pub fn read(b: &[u8], mapped: u64) -> Result<Header, HeaderError> {
        if b.len() < HEADER_LEN {
            return Err(HeaderError::Short);
        }
        if b[0..4] != MAGIC {
            return Err(HeaderError::Magic);
        }
        let theirs = Range {
            min: u16::from_le_bytes([b[6], b[7]]).into(),
            cur: u16::from_le_bytes([b[4], b[5]]).into(),
        };
        if theirs.min > theirs.cur {
            return Err(HeaderError::Version(Outcome::PeerTooNew(theirs)));
        }
        let version = match RANGE.negotiate(theirs) {
            Outcome::Agreed(v) => v,
            o => return Err(HeaderError::Version(o)),
        };
        if u32_at(b, 8) as usize != HEADER_LEN {
            return Err(HeaderError::Length);
        }
        let format = u32_at(b, 12);
        if format != BGRA8_SRGB_PREMUL {
            return Err(HeaderError::Format);
        }
        let (width, height, stride, size) = (u32_at(b, 16), u32_at(b, 20), u32_at(b, 24), u64_at(b, 32));
        fits(width, height, stride, size)?;
        if size > mapped {
            return Err(HeaderError::Mapped);
        }
        Ok(Header { version, format, width, height, stride, size })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(h: &Header) -> Vec<u8> {
        let mut b = vec![0u8; HEADER_LEN];
        h.write(&mut b);
        b
    }

    #[test]
    fn a_header_round_trips() {
        let h = Header::new(1001, 7, Header::needed(1001, 7));
        assert_eq!(h.stride, 4016);
        assert_eq!(Header::read(&buf(&h), h.size), Ok(h));
    }

    #[test]
    fn every_bad_header_is_refused() {
        let h = Header::new(100, 10, Header::needed(100, 10));
        let bad = |at: usize, v: &[u8]| {
            let mut b = buf(&h);
            b[at..at + v.len()].copy_from_slice(v);
            Header::read(&b, h.size)
        };
        assert_eq!(Header::read(&buf(&h)[..10], h.size), Err(HeaderError::Short));
        assert_eq!(bad(0, b"XXXX"), Err(HeaderError::Magic));
        let newer = Range { min: 2, cur: 3 };
        assert_eq!(bad(4, &[3, 0, 2, 0]), Err(HeaderError::Version(Outcome::PeerTooNew(newer))));
        assert_eq!(bad(4, &[0, 0, 0, 0]), Err(HeaderError::Version(Outcome::PeerTooOld(Range { min: 0, cur: 0 }))));
        assert_eq!(bad(8, &32u32.to_le_bytes()), Err(HeaderError::Length));
        assert_eq!(bad(12, &2u32.to_le_bytes()), Err(HeaderError::Format));
        assert_eq!(bad(16, &0u32.to_le_bytes()), Err(HeaderError::Size));
        assert_eq!(bad(20, &(MAX_SIDE + 1).to_le_bytes()), Err(HeaderError::Size));
        assert_eq!(bad(24, &404u32.to_le_bytes()), Err(HeaderError::Stride));
        assert_eq!(bad(24, &384u32.to_le_bytes()), Err(HeaderError::Stride), "under four bytes a pixel");
        assert_eq!(bad(32, &64u64.to_le_bytes()), Err(HeaderError::Mapped), "too small for its rows");
        assert_eq!(Header::read(&buf(&h), h.size - 1), Err(HeaderError::Mapped), "more than is mapped");
        assert_eq!(bad(24, &u32::MAX.to_le_bytes()), Err(HeaderError::Stride));
    }
}
