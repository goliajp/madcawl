//! The Brotli target: a mutated stream decoded whole and in chunks of
//! 1 to `max` bytes into output buffers of changing sizes must give the
//! same bytes, or the same error. The decoder's work is the bytes it
//! produced, bounded by its limit, so the steps check does not apply.

use madcawl_brotli::{Decoder, Status};

use crate::targets::Out;
use crate::wpt::{Feed, Rng};

/// Output limit per input: small enough to keep runs fast, large enough
/// for every seed
const LIMIT: usize = 1 << 20;

pub const PIECES: &[&[u8]] = &[
    b"\x00",
    b"\xff",
    b"\x80",
    b"\x01",
    b"\x1b",
    b"\x3b",
    b"\xa1\x00",
    b"\x8b\x02\x80",
];

pub fn corpus() -> Vec<Vec<u8>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/madcawl-brotli/tests/data");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".compressed")
        })
        .collect();
    files.sort();
    let mut out: Vec<Vec<u8>> = files.iter().map(|p| std::fs::read(p).unwrap()).collect();
    out.dedup();
    out
}

pub fn decode(input: &[u8], feed: Feed) -> Out {
    // whole: one piece and a large output buffer
    let (mut rng, max) = match feed {
        Feed::Whole => (None, 0),
        Feed::Bytewise => (Some(Rng(1)), 1),
        Feed::Chunks { seed, max } => (Some(Rng(seed | 1)), max),
    };
    let mut d = Decoder::new(LIMIT);
    let mut out = Vec::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut at = 0;
    let result = loop {
        let (n, size) = match &mut rng {
            None => (input.len() - at, buf.len()),
            Some(r) => (
                (1 + r.next() as usize % max).min(input.len() - at),
                1 + r.next() as usize % buf.len(),
            ),
        };
        let p = match d.decompress(&input[at..at + n], &mut buf[..size]) {
            Ok(p) => p,
            Err(e) => {
                // how much was handed out before the error depends on the buffers
                out.clear();
                break format!("error {e}");
            }
        };
        out.extend_from_slice(&buf[..p.produced]);
        at += p.consumed;
        match p.status {
            Status::Done => break "done".to_string(),
            Status::NeedInput if at == input.len() => break "truncated".to_string(),
            _ => {}
        }
    };
    out.extend_from_slice(result.as_bytes());
    Out {
        flat: out,
        steps: 0,
        audit: None,
    }
}
