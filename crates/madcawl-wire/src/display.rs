//! A display channel's frames, between the display end and the content
//! process of one pane. The display end makes the shared-memory buffers
//! and sends them in `SURFACE`, their descriptors with the frame; the
//! content process draws the viewport into a buffer the display end is
//! not reading, sets its generation and sends `COMMIT`; the display end
//! answers `RELEASED` once it no longer reads a buffer, which may then
//! be drawn into again. Each side is the other's trust boundary: every
//! frame is checked here before anything it names is used.

use crate::frame::Frame;
use crate::shm::{self, HeaderError};
use crate::version::{Range, num32};
use crate::{Bad, tag};

/// The display channel versions this build speaks.
pub const RANGE: Range = Range::new(2, 2);
/// The most buffers a surface has.
pub const MAX_BUFFERS: usize = 2;
/// The largest document size a commit may state, CSS px.
pub const MAX_DOC: u32 = 1 << 24;

pub mod kind {
    /// Both ways: `MIN`, `CUR`; the answer adds `VERSION` when the
    /// ranges meet
    pub const HELLO: u16 = 1;
    /// Display end to content: `EPOCH`, `SCALE` and a `BUFFER` field a
    /// buffer, each buffer's descriptor with the frame, in the same
    /// order; each surface on a channel has a larger epoch than the last
    pub const SURFACE: u16 = 2;
    /// Content to display end: `EPOCH` of the surface drawn into, `SEQ`,
    /// `BUFFER` (its index), `DIRTY`, `DOC_WIDTH`, `DOC_HEIGHT`
    pub const COMMIT: u16 = 3;
    /// Display end to content: `SEQ` of a commit whose buffer it no
    /// longer reads; one sent before the latest surface came frees
    /// nothing and is passed over, one never sent is an error
    pub const RELEASED: u16 = 4;
}

/// One buffer of a surface: its size in bytes, header included, and the
/// pixels it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Buffer {
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

impl Buffer {
    fn encode(&self) -> [u8; 20] {
        let mut b = [0u8; 20];
        b[0..8].copy_from_slice(&self.bytes.to_le_bytes());
        b[8..12].copy_from_slice(&self.width.to_le_bytes());
        b[12..16].copy_from_slice(&self.height.to_le_bytes());
        b[16..20].copy_from_slice(&self.stride.to_le_bytes());
        b
    }

    fn decode(b: &[u8]) -> Result<Buffer, Bad> {
        let b: &[u8; 20] = b.try_into().map_err(|_| Bad::Value(tag::BUFFER))?;
        let u = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().expect("four bytes"));
        let buf = Buffer { bytes: u64::from_le_bytes(b[0..8].try_into().expect("eight bytes")), width: u(8), height: u(12), stride: u(16) };
        shm::fits(buf.width, buf.height, buf.stride, buf.bytes).map_err(|_| Bad::Value(tag::BUFFER))?;
        Ok(buf)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Surface {
    pub epoch: u64,
    /// Device pixels per CSS px, in hundredths
    pub scale: u32,
    pub buffers: Vec<Buffer>,
}

impl Surface {
    pub fn frame(&self) -> Frame {
        let f = Frame::new(kind::SURFACE).num(tag::EPOCH, self.epoch).num(tag::SCALE, self.scale.into());
        self.buffers.iter().fold(f, |f, b| f.bytes(tag::BUFFER, &b.encode()))
    }

    /// The surface `f` states, which came with `fds` descriptors: one a
    /// buffer, every buffer the same size in pixels.
    pub fn from_frame(f: &Frame, fds: usize) -> Result<Surface, Bad> {
        if f.kind != kind::SURFACE {
            return Err(Bad::Kind(f.kind));
        }
        let epoch = f.get_num(tag::EPOCH).ok_or(Bad::Missing(tag::EPOCH))?;
        let scale = num32(f, tag::SCALE)?;
        if !crate::host::SCALE_RANGE.contains(&scale) {
            return Err(Bad::Value(tag::SCALE));
        }
        let n = f.fields_of(tag::BUFFER).count();
        if n == 0 {
            return Err(Bad::Missing(tag::BUFFER));
        }
        if n > MAX_BUFFERS {
            return Err(Bad::Value(tag::BUFFER));
        }
        if n != fds {
            return Err(Bad::Fds);
        }
        let buffers = f.fields_of(tag::BUFFER).map(Buffer::decode).collect::<Result<Vec<_>, _>>()?;
        if buffers.iter().any(|b| (b.width, b.height) != (buffers[0].width, buffers[0].height)) {
            return Err(Bad::Value(tag::BUFFER));
        }
        Ok(Surface { epoch, scale, buffers })
    }

    /// Whether a buffer's header, read from memory `mapped` bytes long,
    /// is the one this surface states for buffer `i`.
    pub fn check_header(&self, i: usize, head: &[u8], mapped: u64) -> Result<(), HeaderError> {
        let h = shm::Header::read(head, mapped)?;
        let b = self.buffers[i];
        match (h.width, h.height, h.stride, h.size) == (b.width, b.height, b.stride, b.bytes) && mapped >= b.bytes {
            true => Ok(()),
            false => Err(HeaderError::Mapped),
        }
    }
}

/// A rectangle of device pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PxRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Commit {
    /// The surface drawn into
    pub epoch: u64,
    pub seq: u64,
    pub buffer: u32,
    pub dirty: PxRect,
    /// The document's size, CSS px
    pub doc_width: u32,
    pub doc_height: u32,
}

/// Why a commit's pixels are not used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitError {
    /// It was drawn into another surface than the one held
    Epoch,
    /// Its sequence is not after the last one taken
    Seq,
    /// It names a buffer the surface does not have
    NoBuffer,
    /// It names the buffer the display end is reading
    Busy,
    /// The dirty rectangle is empty or reaches past the buffer
    Dirty,
    Doc,
}

impl Commit {
    pub fn frame(&self) -> Frame {
        let d = self.dirty;
        let mut r = [0u8; 16];
        for (i, v) in [d.x, d.y, d.w, d.h].into_iter().enumerate() {
            r[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        Frame::new(kind::COMMIT)
            .num(tag::EPOCH, self.epoch)
            .num(tag::SEQ, self.seq)
            .num(tag::BUFFER, self.buffer.into())
            .bytes(tag::DIRTY, &r)
            .num(tag::DOC_WIDTH, self.doc_width.into())
            .num(tag::DOC_HEIGHT, self.doc_height.into())
    }

    pub fn from_frame(f: &Frame) -> Result<Commit, Bad> {
        if f.kind != kind::COMMIT {
            return Err(Bad::Kind(f.kind));
        }
        let r = f.field(tag::DIRTY).ok_or(Bad::Missing(tag::DIRTY))?;
        let r: &[u8; 16] = r.try_into().map_err(|_| Bad::Value(tag::DIRTY))?;
        let u = |i: usize| u32::from_le_bytes(r[i * 4..i * 4 + 4].try_into().expect("four bytes"));
        Ok(Commit {
            epoch: f.get_num(tag::EPOCH).ok_or(Bad::Missing(tag::EPOCH))?,
            seq: f.get_num(tag::SEQ).ok_or(Bad::Missing(tag::SEQ))?,
            buffer: num32(f, tag::BUFFER)?,
            dirty: PxRect { x: u(0), y: u(1), w: u(2), h: u(3) },
            doc_width: num32(f, tag::DOC_WIDTH)?,
            doc_height: num32(f, tag::DOC_HEIGHT)?,
        })
    }

    /// Whether the display end may use this commit: drawn into
    /// `surface`, after commit `last` (0 before the first), on a buffer
    /// other than the one it is `reading`, its dirty rectangle within the
    /// buffer. A commit drawn into an earlier surface may still come
    /// after a new one is sent; it is refused, not an error of the peer.
    pub fn check(&self, surface: &Surface, last: u64, reading: Option<u32>) -> Result<(), CommitError> {
        if self.epoch != surface.epoch {
            return Err(CommitError::Epoch);
        }
        if self.seq <= last {
            return Err(CommitError::Seq);
        }
        let Some(b) = surface.buffers.get(self.buffer as usize) else {
            return Err(CommitError::NoBuffer);
        };
        if reading == Some(self.buffer) {
            return Err(CommitError::Busy);
        }
        let d = self.dirty;
        let inside = |at: u32, len: u32, max: u32| len > 0 && at.checked_add(len).is_some_and(|end| end <= max);
        if !inside(d.x, d.w, b.width) || !inside(d.y, d.h, b.height) {
            return Err(CommitError::Dirty);
        }
        if self.doc_width > MAX_DOC || self.doc_height > MAX_DOC {
            return Err(CommitError::Doc);
        }
        Ok(())
    }
}

/// `RELEASED` for commit `seq`.
pub fn released(seq: u64) -> Frame {
    Frame::new(kind::RELEASED).num(tag::SEQ, seq)
}

/// The sequence a `RELEASED` names.
pub fn released_seq(f: &Frame) -> Result<u64, Bad> {
    match f.kind {
        kind::RELEASED => f.get_num(tag::SEQ).ok_or(Bad::Missing(tag::SEQ)),
        k => Err(Bad::Kind(k)),
    }
}

#[cfg(test)]
#[path = "display_tests.rs"]
mod tests;
