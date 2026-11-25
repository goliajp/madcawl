//! A streaming Brotli decoder (RFC 7932).
//!
//! Input may arrive in pieces of any size, down to one byte; output is
//! handed out into whatever buffer the caller passes. Memory is the
//! sliding window (at most 16 MiB, less when the output limit or the
//! stream is smaller), a 64 KiB input buffer and the tables of the
//! current meta-block; nothing is allocated per byte.
//!
//! The decoder is given a limit on its total output: a stream that
//! declares more is an error, so a small body cannot expand without
//! bound. The large-window extension is not supported.
//!
//! ```
//! let out = madcawl_brotli::decompress(&[0x21, 0x04, 0x00, 0x04, 0x68, 0x69, 0x03], 100).unwrap();
//! assert_eq!(out, b"hi");
//! ```

mod bits;
mod data;
mod decoder;
mod dict;
mod header;
mod meta;
mod prefix;
mod ring;
mod tables;
mod transforms;

pub use decoder::Decoder;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The stream is not valid Brotli; the text says what was wrong
    Format(&'static str),
    /// The stream declares more output than the limit
    TooLarge,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Format(why) => write!(f, "invalid brotli stream: {why}"),
            Error::TooLarge => f.write_str("brotli output over the limit"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// All input was taken and the stream has not ended
    NeedInput,
    /// The output buffer is full and there is more to hand out
    NeedOutput,
    /// The stream ended and everything was handed out
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Input bytes taken. Once the stream has ended the rest of this call's
    /// input is not read, but bytes buffered in earlier calls may lie past
    /// the end and are still counted; `Decoder::stream_len` gives the exact
    /// length of the stream
    pub consumed: usize,
    pub produced: usize,
    pub status: Status,
}

/// Decodes a whole stream of at most `limit` output bytes
pub fn decompress(input: &[u8], limit: usize) -> Result<Vec<u8>, Error> {
    let mut d = Decoder::new(limit);
    let mut out = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let mut at = 0;
    loop {
        let p = d.decompress(&input[at..], &mut chunk)?;
        at += p.consumed;
        out.extend_from_slice(&chunk[..p.produced]);
        match p.status {
            Status::Done => return Ok(out),
            Status::NeedInput => return Err(Error::Format("truncated stream")),
            Status::NeedOutput => {}
        }
    }
}
