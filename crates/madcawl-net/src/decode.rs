//! `Content-Encoding` decoding with a cap on the output, so a small body
//! cannot expand without bound.

use crate::error::NetError;
use torajs_flate::cpu::CpuFeatures;
use torajs_flate::inflate::Inflate;
use torajs_flate::{Flush, Format, Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coding {
    Identity,
    Gzip,
    Deflate,
}

/// The codings of a `Content-Encoding` value in the order they were
/// applied, or an error naming one this client cannot undo.
pub fn codings(value: &[u8]) -> Result<Vec<Coding>, NetError> {
    let text = std::str::from_utf8(value).map_err(|_| NetError::Decode("encoding not ascii".into()))?;
    let mut out = Vec::new();
    for c in text.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        out.push(match c.to_ascii_lowercase().as_str() {
            "identity" => Coding::Identity,
            "gzip" | "x-gzip" => Coding::Gzip,
            "deflate" => Coding::Deflate,
            other => return Err(NetError::Decode(format!("unsupported coding {other}"))),
        });
        if out.len() > 4 {
            return Err(NetError::Decode("too many codings".into()));
        }
    }
    Ok(out)
}

/// Undoes `codings` (last applied first) on `body`; no stage may produce
/// more than `cap` bytes.
pub fn decode(body: Vec<u8>, codings: &[Coding], cap: usize) -> Result<Vec<u8>, NetError> {
    let mut cur = body;
    for c in codings.iter().rev() {
        cur = match c {
            Coding::Identity => cur,
            Coding::Gzip => inflate(&cur, Format::Gzip, cap)?,
            Coding::Deflate => {
                // "deflate" means zlib, but some servers send raw deflate
                let zlib = cur.len() >= 2 && cur[0] & 0x0f == 8 && (u16::from(cur[0]) << 8 | u16::from(cur[1])) % 31 == 0;
                inflate(&cur, if zlib { Format::Zlib } else { Format::Raw }, cap)?
            }
        };
    }
    Ok(cur)
}

/// The state is 8-byte aligned memory the decoder borrows.
struct State(Vec<u64>);

impl State {
    fn new() -> Self {
        State(vec![0u64; Inflate::SIZE.div_ceil(8)])
    }

    fn inflate(&mut self) -> &mut Inflate {
        // SAFETY: a u64 buffer viewed as bytes of the same length
        let bytes = unsafe { std::slice::from_raw_parts_mut(self.0.as_mut_ptr().cast::<u8>(), self.0.len() * 8) };
        // SAFETY: zeroed, 8-byte aligned and SIZE bytes long
        unsafe { Inflate::from_zeroed(bytes) }
    }
}

fn inflate(input: &[u8], format: Format, cap: usize) -> Result<Vec<u8>, NetError> {
    let mut state = State::new();
    let d = state.inflate();
    d.reset(format, CpuFeatures::compile_time());
    let mut out = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let mut at = 0;
    loop {
        let p = d
            .inflate(&input[at..], &mut chunk, Flush::Finish)
            .map_err(|e| NetError::Decode(format!("{e:?}")))?;
        at += p.consumed;
        if out.len() + p.produced > cap {
            return Err(NetError::TooLarge("decoded body"));
        }
        out.extend_from_slice(&chunk[..p.produced]);
        match p.status {
            Status::Done => return Ok(out),
            Status::NeedOutput => {}
            Status::NeedInput => return Err(NetError::Decode("truncated stream".into())),
        }
    }
}
