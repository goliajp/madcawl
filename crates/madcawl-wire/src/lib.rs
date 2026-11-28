//! What crosses a process boundary between madcawl and the program that
//! shows its pages, with no dependency but the standard library:
//!
//! - `frame`: the length-prefixed frame every channel speaks
//! - `host`: a host connection's frames beyond the agent protocol's, for
//!   a connection playing the display end (`HELLO`, `PANE_OPEN`,
//!   `PANE_OPENED`)
//! - `display`: a display channel's frames, between the display end and
//!   the content process of one pane (`HELLO`, `SURFACE`, `COMMIT`,
//!   `RELEASED`), with the checks a commit must pass before its pixels
//!   are used
//! - `version`: version ranges and what two of them come to
//! - `shm`: the header of a shared-memory surface and its checks
//! - `os`: shared memory and passing descriptors over a Unix socket
//!
//! Every reader treats the other side as untrusted: lengths, counts and
//! rectangles are checked before anything is allocated or touched, and
//! kinds and tags it does not know are skipped.

pub mod display;
pub mod frame;
pub mod host;
pub mod os;
pub mod shm;
pub mod version;

pub use frame::{Frame, FrameError, MAX_FIELDS, MAX_FRAME};
pub use version::{Hello, Outcome, Range};

/// The largest side of a surface, in device pixels.
pub const MAX_SIDE: u32 = 16384;

/// The tags of the frames here. Those the agent protocol has too mean
/// the same there.
pub mod tag {
    pub const TEXT: u8 = 7;
    pub const SEQ: u8 = 11;
    pub const CODE: u8 = 14;
    pub const PAGE: u8 = 15;
    pub const URL: u8 = 16;
    /// CSS px
    pub const WIDTH: u8 = 17;
    pub const HEIGHT: u8 = 18;
    /// Device pixels per CSS px, in hundredths
    pub const SCALE: u8 = 36;
    pub const MIN: u8 = 64;
    pub const CUR: u8 = 65;
    pub const VERSION: u8 = 66;
    pub const ROLE: u8 = 67;
    /// The display end's own number for the pane
    pub const SESSION: u8 = 68;
    pub const PID: u8 = 69;
    /// One shared-memory buffer of a surface, repeated
    pub const BUFFER: u8 = 70;
    /// A rectangle of device pixels: x, y, width, height
    pub const DIRTY: u8 = 71;
    /// The document's size, CSS px
    pub const DOC_WIDTH: u8 = 72;
    pub const DOC_HEIGHT: u8 = 73;
    /// A surface's number on its display channel, larger each time
    pub const EPOCH: u8 = 74;
}

/// Why a frame was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bad {
    /// A field the frame must have is not there
    Missing(u8),
    /// A field is out of its range or the wrong size
    Value(u8),
    /// Not the kind expected
    Kind(u16),
    /// The descriptors that came with it are not the ones it names
    Fds,
}

impl std::fmt::Display for Bad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Bad::Missing(t) => write!(f, "field {t} is missing"),
            Bad::Value(t) => write!(f, "field {t} is out of range"),
            Bad::Kind(k) => write!(f, "a frame of kind {k} was not expected"),
            Bad::Fds => f.write_str("the descriptors do not match the frame"),
        }
    }
}

impl std::error::Error for Bad {}
