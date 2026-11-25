//! `Content-Encoding` decoding with a cap on the output, so a small body
//! cannot expand without bound.

use crate::error::NetError;
use torajs_flate::cpu::CpuFeatures;
use torajs_flate::inflate::Inflate;
use torajs_flate::{Flush, Format, Status};

mod brotli;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coding {
    Identity,
    Gzip,
    Deflate,
    Brotli,
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
            "br" => Coding::Brotli,
            other => return Err(NetError::Decode(format!("unsupported coding {other}"))),
        });
        if out.len() > 4 {
            return Err(NetError::Decode("too many codings".into()));
        }
    }
    Ok(out)
}

/// Undoes `codings` (last applied first) on a whole `body`; the output
/// may be at most `cap` bytes.
pub fn decode(body: &[u8], codings: &[Coding], cap: usize) -> Result<Vec<u8>, NetError> {
    let mut d = Decoder::new(codings, cap);
    let mut out = Vec::new();
    d.push(body, &mut |b| {
        out.extend_from_slice(b);
        Ok(())
    })?;
    d.finish(&mut |b| {
        out.extend_from_slice(b);
        Ok(())
    })?;
    Ok(out)
}

pub type Emit<'a> = dyn FnMut(&[u8]) -> Result<(), NetError> + 'a;

/// Undoes `codings` on a body that arrives in pieces, handing the output
/// on as it is produced; each layer may produce at most `cap` bytes.
pub struct Decoder {
    stages: Vec<Stage>,
}

impl Decoder {
    pub fn new(codings: &[Coding], cap: usize) -> Self {
        let stages = codings
            .iter()
            .rev()
            .filter_map(|c| match c {
                Coding::Identity => None,
                Coding::Gzip => Some(Stage::new(Some(Format::Gzip), cap)),
                Coding::Deflate => Some(Stage::new(None, cap)),
                Coding::Brotli => Some(Stage::brotli(cap)),
            })
            .collect();
        Decoder { stages }
    }

    pub fn push(&mut self, input: &[u8], out: &mut Emit<'_>) -> Result<(), NetError> {
        self.run(input, false, out)
    }

    /// No more input: a stream that has not ended is an error.
    pub fn finish(&mut self, out: &mut Emit<'_>) -> Result<(), NetError> {
        self.run(&[], true, out)
    }

    fn run(&mut self, input: &[u8], finish: bool, out: &mut Emit<'_>) -> Result<(), NetError> {
        feed(&mut self.stages, input, finish, out)
    }
}

/// Runs `input` through the stages in turn. Every stage's output counts
/// against the cap, not only the last one's, and a stage that ended takes
/// no more: output an earlier stage makes after it is an error, so a
/// layer cannot be made to inflate data nobody reads.
fn feed(stages: &mut [Stage], input: &[u8], finish: bool, out: &mut Emit<'_>) -> Result<(), NetError> {
    let Some((first, rest)) = stages.split_first_mut() else {
        return if input.is_empty() { Ok(()) } else { out(input) };
    };
    first.run(input, finish, &mut |b| {
        if rest.first().is_some_and(|s| s.done) {
            return Err(NetError::Decode("data after the end of an inner coding".into()));
        }
        feed(rest, b, false, out)
    })?;
    if finish {
        feed(rest, &[], true, out)?;
    }
    Ok(())
}

/// The state is 8-byte aligned memory the decoder borrows, zeroed until
/// the first borrow.
struct State {
    mem: Vec<u64>,
    borrowed: bool,
}

impl State {
    fn new() -> Self {
        State { mem: vec![0u64; Inflate::SIZE.div_ceil(8)], borrowed: false }
    }

    fn inflate(&mut self) -> &mut Inflate {
        // SAFETY: a u64 buffer viewed as bytes of the same length
        let bytes = unsafe { std::slice::from_raw_parts_mut(self.mem.as_mut_ptr().cast::<u8>(), self.mem.len() * 8) };
        if std::mem::replace(&mut self.borrowed, true) {
            // SAFETY: written only through the Inflate handed out before
            unsafe { Inflate::from_bytes_mut(bytes) }.expect("the layout was checked on the first borrow")
        } else {
            // SAFETY: zeroed, 8-byte aligned and SIZE bytes long
            unsafe { Inflate::from_zeroed(bytes) }
        }
    }
}

/// One inflate pass. `format` is `None` for "deflate" until its first two
/// bytes say whether it is zlib or, as some servers send, raw deflate.
struct Stage {
    state: State,
    format: Option<Format>,
    pending: Vec<u8>,
    done: bool,
    chunk: Vec<u8>,
    produced: usize,
    cap: usize,
    brotli: Option<Box<madcawl_brotli::Decoder>>,
}

fn is_zlib(b: &[u8]) -> bool {
    b.len() >= 2 && b[0] & 0x0f == 8 && (u16::from(b[0]) << 8 | u16::from(b[1])) % 31 == 0
}

impl Stage {
    fn new(format: Option<Format>, cap: usize) -> Self {
        let chunk = vec![0u8; 32 * 1024];
        let mut s = Stage {
            state: State::new(),
            format,
            pending: Vec::new(),
            done: false,
            chunk,
            produced: 0,
            cap,
            brotli: None,
        };
        if let Some(f) = format {
            s.state.inflate().reset(f, CpuFeatures::compile_time());
        }
        s
    }

    fn run(&mut self, input: &[u8], finish: bool, emit: &mut Emit<'_>) -> Result<(), NetError> {
        // bytes after the end of the stream are ignored
        if self.done {
            return Ok(());
        }
        if self.brotli.is_some() {
            return self.run_brotli(input, finish, emit);
        }
        let sniffed;
        let mut input = input;
        if self.format.is_none() {
            self.pending.extend_from_slice(input);
            if self.pending.len() < 2 && !finish {
                return Ok(());
            }
            let f = if is_zlib(&self.pending) { Format::Zlib } else { Format::Raw };
            self.state.inflate().reset(f, CpuFeatures::compile_time());
            self.format = Some(f);
            sniffed = std::mem::take(&mut self.pending);
            input = &sniffed;
        }
        let flush = if finish { Flush::Finish } else { Flush::More };
        let mut at = 0;
        loop {
            let p = self
                .state
                .inflate()
                .inflate(&input[at..], &mut self.chunk, flush)
                .map_err(|e| NetError::Decode(format!("{e:?}")))?;
            at += p.consumed;
            self.produced += p.produced;
            if self.produced > self.cap {
                return Err(NetError::TooLarge("decoded body"));
            }
            if p.produced > 0 {
                emit(&self.chunk[..p.produced])?;
            }
            match p.status {
                Status::Done => {
                    self.done = true;
                    return Ok(());
                }
                Status::NeedOutput => {}
                Status::NeedInput if finish => return Err(NetError::Decode("truncated stream".into())),
                Status::NeedInput => return Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_in_one_byte_pieces_decodes_like_a_whole_one() {
        // "hello" as zlib, then as raw deflate
        let zlib = [0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00, 0x06, 0x2c, 0x02, 0x15];
        let raw = &zlib[2..zlib.len() - 4];
        for body in [&zlib[..], raw] {
            assert_eq!(decode(body, &[Coding::Deflate], 100).unwrap(), b"hello");
            let mut d = Decoder::new(&[Coding::Deflate], 100);
            let mut out = Vec::new();
            for b in body {
                d.push(std::slice::from_ref(b), &mut |x| {
                    out.extend_from_slice(x);
                    Ok(())
                })
                .unwrap();
            }
            d.finish(&mut |x| {
                out.extend_from_slice(x);
                Ok(())
            })
            .unwrap();
            assert_eq!(out, b"hello");
        }
        assert_eq!(decode(&zlib, &[Coding::Deflate], 4), Err(NetError::TooLarge("decoded body")));
        assert!(matches!(decode(&zlib[..6], &[Coding::Deflate], 100), Err(NetError::Decode(_))));
    }
}
