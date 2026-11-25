//! The reference implementation's test vectors (see `data/SOURCE`),
//! decoded whole, one byte at a time and in uneven pieces.

use madcawl_brotli::{Decoder, Error, Status, decompress};

fn data(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

struct Vector {
    name: String,
    compressed: Vec<u8>,
    original: Vec<u8>,
}

fn vectors() -> Vec<Vector> {
    let mut v = Vec::new();
    let mut add = |name: String, original: Vec<u8>| {
        let compressed = data(&name);
        v.push(Vector { name, compressed, original });
    };
    for n in [
        "x",
        "xyzzy",
        "64x",
        "10x10y",
        "quickfox",
        "ukkonooa",
        "monkey",
        "cp852-utf8",
        "cp1251-utf16le",
        "random_org_10k.bin",
        "asyoulik.txt",
    ] {
        add(format!("{n}.compressed"), data(n));
    }
    for k in 0..4 {
        add(format!("x.compressed.{k:02}"), data("x"));
    }
    add("empty.compressed".into(), Vec::new());
    for k in 0..18 {
        add(format!("empty.compressed.{k:02}"), Vec::new());
    }
    let zeros = vec![0u8; 262_144];
    add("zeros.compressed".into(), zeros.clone());
    add("zerosukkanooa.compressed".into(), [zeros, data("ukkonooa")].concat());
    let repeated = data("quickfox").repeat(4096);
    add("quickfox_repeated.compressed".into(), repeated.clone());
    add("quickfox_repeated.w10.compressed".into(), repeated);
    let backward = [vec![0u8; 256], vec![b'X'; 65_280], vec![0u8; 256]].concat();
    add("backward65536.compressed".into(), backward);
    add("asyoulik.txt.w10.compressed".into(), data("asyoulik.txt"));
    add("monkey.w24.compressed".into(), data("monkey"));
    v
}

/// Decodes `input` handing it over in pieces of `piece()` bytes, with an
/// output buffer of `out_size` bytes
fn pieces(input: &[u8], limit: usize, mut piece: impl FnMut() -> usize, out_size: usize) -> Result<Vec<u8>, Error> {
    let mut d = Decoder::new(limit);
    let mut out = Vec::new();
    let mut buf = vec![0u8; out_size];
    let mut at = 0;
    loop {
        let n = piece().min(input.len() - at);
        let p = d.decompress(&input[at..at + n], &mut buf)?;
        out.extend_from_slice(&buf[..p.produced]);
        at += p.consumed;
        match p.status {
            Status::Done => return Ok(out),
            Status::NeedInput if at == input.len() => return Err(Error::Format("truncated stream")),
            _ => {}
        }
    }
}

/// xorshift, so the piece sizes are the same on every run
fn rng(mut s: u64) -> impl FnMut() -> u64 {
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    }
}

#[test]
fn every_vector_decodes_whole() {
    for v in vectors() {
        let got = decompress(&v.compressed, v.original.len()).unwrap_or_else(|e| panic!("{}: {e}", v.name));
        assert!(got == v.original, "{}: wrong output", v.name);
    }
}

#[test]
fn every_vector_decodes_one_byte_at_a_time() {
    for v in vectors() {
        let got = pieces(&v.compressed, v.original.len(), || 1, 7).unwrap_or_else(|e| panic!("{}: {e}", v.name));
        assert!(got == v.original, "{}: wrong output", v.name);
    }
}

#[test]
fn every_vector_decodes_in_uneven_pieces() {
    let mut r = rng(0x9e37_79b9_7f4a_7c15);
    for v in vectors() {
        for _ in 0..3 {
            let out_size = 1 + (r() % 5000) as usize;
            let mut pr = rng(r() | 1);
            let got = pieces(&v.compressed, usize::MAX, || 1 + (pr() % 700) as usize, out_size)
                .unwrap_or_else(|e| panic!("{}: {e}", v.name));
            assert!(got == v.original, "{}: wrong output", v.name);
        }
    }
}

#[test]
fn output_over_the_limit_is_an_error_not_a_truncation() {
    for v in vectors().into_iter().filter(|v| !v.original.is_empty()) {
        assert_eq!(decompress(&v.compressed, v.original.len() - 1), Err(Error::TooLarge), "{}", v.name);
        assert_eq!(pieces(&v.compressed, v.original.len() - 1, || 1, 64), Err(Error::TooLarge), "{}", v.name);
    }
}

#[test]
fn every_cut_short_vector_is_an_error() {
    for v in vectors() {
        let n = v.compressed.len();
        let step = n.div_ceil(400).max(1);
        for cut in (0..n).step_by(step) {
            let r = decompress(&v.compressed[..cut], usize::MAX);
            assert!(r.is_err(), "{} cut at {cut} of {n} decoded", v.name);
        }
    }
}

#[test]
fn bytes_after_the_end_are_not_read() {
    let mut input = data("x.compressed");
    let n = input.len();
    input.extend_from_slice(b"trailing garbage");
    assert_eq!(decompress(&input, 10).unwrap(), b"X");
    let mut d = Decoder::new(10);
    let mut out = [0u8; 4];
    let p = d.decompress(&input, &mut out).unwrap();
    assert_eq!((p.status, p.produced, &out[..1]), (Status::Done, 1, &b"X"[..]));
    assert_eq!(p.consumed, n);
    assert_eq!(d.stream_len(), Some(n));
    // the doc example with four bytes after it, and the same split so the
    // end falls in the second call
    let mut input = vec![0x21, 0x04, 0x00, 0x04, 0x68, 0x69, 0x03, 1, 2, 3, 4];
    let p = Decoder::new(10).decompress(&input, &mut out).unwrap();
    assert_eq!((p.consumed, p.produced, p.status), (7, 2, Status::Done));
    let mut d = Decoder::new(10);
    let p = d.decompress(&input[..5], &mut out).unwrap();
    assert_eq!((p.consumed, p.produced, p.status), (5, 1, Status::NeedInput));
    let p = d.decompress(&input[5..], &mut out).unwrap();
    assert_eq!((p.consumed, p.produced, p.status), (2, 1, Status::Done));
    input.truncate(7);
    let p = Decoder::new(10).decompress(&input, &mut out).unwrap();
    assert_eq!((p.consumed, p.status), (7, Status::Done));
}

#[test]
fn stream_len_is_exact_when_the_end_comes_in_a_later_call() {
    // 5000 'a' through `brotli -c -w 10`, then four bytes after the end
    let stream = [0xa1, 0x38, 0x9c, 0xc0, 0x2f, 0x11, 0x16, 0x8f, 0x05, 0x02, 0x41, 0x0b, 0x00];
    let input = [&stream[..], &[1, 2, 3, 4]].concat();
    let mut d = Decoder::new(5000);
    let mut out = [0u8; 1];
    let (mut at, mut produced) = (0, 0);
    loop {
        assert_eq!(d.stream_len(), None);
        let p = d.decompress(&input[at..], &mut out).unwrap();
        at += p.consumed;
        produced += p.produced;
        assert_eq!(&out[..p.produced], &b"a"[..p.produced]);
        if p.status == Status::Done {
            break;
        }
    }
    assert_eq!(produced, 5000);
    // the first call buffered all 17 bytes, so the summed `consumed` is past the end
    assert_eq!(at, input.len());
    assert_eq!(d.stream_len(), Some(stream.len()));
}

#[test]
fn stream_len_with_bytes_after_the_end_in_pieces() {
    let mut r = rng(0x2545_f491_4f6c_dd1d);
    for v in vectors() {
        let input = [&v.compressed[..], b"trailing junk after the end"].concat();
        for _ in 0..3 {
            let mut d = Decoder::new(usize::MAX);
            let mut buf = vec![0u8; 1 + (r() % 3000) as usize];
            let mut pr = rng(r() | 1);
            let (mut at, mut out) = (0, Vec::new());
            loop {
                let n = (1 + (pr() % 700) as usize).min(input.len() - at);
                let p = d.decompress(&input[at..at + n], &mut buf).unwrap_or_else(|e| panic!("{}: {e}", v.name));
                out.extend_from_slice(&buf[..p.produced]);
                at += p.consumed;
                if p.status == Status::Done {
                    break;
                }
                assert!(at < input.len() || p.status == Status::NeedOutput, "{}: truncated", v.name);
            }
            assert!(out == v.original, "{}: wrong output", v.name);
            assert_eq!(d.stream_len(), Some(v.compressed.len()), "{}", v.name);
        }
    }
}

#[test]
fn an_error_sticks() {
    let mut d = Decoder::new(10);
    let mut out = [0u8; 4];
    // window bits pattern 0010001 is reserved for the large-window extension
    let e = d.decompress(&[0x11], &mut out).unwrap_err();
    assert_eq!(d.decompress(&data("x.compressed"), &mut out), Err(e));
}
