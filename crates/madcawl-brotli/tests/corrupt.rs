//! Streams written bit by bit: small valid ones, each rule of the format
//! broken once, and every single-bit flip of a few vectors.

use madcawl_brotli::{Error, decompress};

/// Writes fields least significant bit first, as the format reads them
#[derive(Default)]
struct W {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl W {
    fn bits(mut self, v: u64, n: u32) -> Self {
        self.acc |= v << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
        self
    }

    fn done(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// Window bits 16, then a last meta-block of `len` bytes with one block
/// type per category, no direct distance codes, one tree per kind, a
/// one-symbol literal code ('a') and command code `cmd`, and a distance
/// code of `dist` over the 64-symbol alphabet
fn last_block(len: u64, cmd: u64, dist: u64) -> W {
    block_start(len).bits(1, 2).bits(0, 2).bits(cmd, 10).bits(1, 2).bits(0, 2).bits(dist, 6)
}

/// [`last_block`] up to the command code
fn block_start(len: u64) -> W {
    W::default()
        .bits(0, 1) // window bits 16
        .bits(1, 1) // last
        .bits(0, 1) // not empty
        .bits(0, 2) // 4 nibbles
        .bits(len - 1, 16)
        .bits(0, 3) // one literal, command and distance block type
        .bits(0, 6) // NPOSTFIX 0, NDIRECT 0
        .bits(0, 2) // context mode LSB6
        .bits(0, 2) // one literal tree, one distance tree
        .bits(1, 2) // simple code with one symbol
        .bits(0, 2)
        .bits(u64::from(b'a'), 8)
}

fn format_error(input: &[u8]) -> bool {
    matches!(decompress(input, 1 << 20), Err(Error::Format(_)))
}

#[test]
fn crafted_streams_decode() {
    // insert 3 literals, copy 4 from distance 1: "aaa" then "aaaa"
    let s = last_block(7, 128 | 3 << 3 | 2, 16).bits(0, 1).done();
    assert_eq!(decompress(&s, 100).unwrap(), b"aaaaaaa");
    // no literals, then a copy of 4 from beyond the output: word 0 of length 4
    let s = last_block(4, 128 | 2, 16).bits(0, 1).done();
    assert_eq!(decompress(&s, 100).unwrap(), b"time");
    // every window size: an empty last meta-block after each pattern
    for (pattern, n) in [(0u64, 1), (0b0000001, 7), (0b0100001, 7), (0b1110001, 7), (0b0011, 4), (0b1111, 4)] {
        let s = W::default().bits(pattern, n).bits(0b11, 2).done();
        assert_eq!(decompress(&s, 10).unwrap(), b"", "{pattern:b}");
    }
}

#[test]
fn commands_that_output_nothing_decode() {
    // a meta-block of one byte with two commands: command 130 (no
    // literals, copy 4) at distance code 42 with 14 extra bits 2052,
    // which is dictionary id 34 << 10, transform 34 (OmitFirst4) of a
    // 4-byte word, so nothing; then command 8 (insert 1) gives 'a'
    let s = block_start(1)
        .bits(1, 2) // simple code with two symbols, 8 then 130
        .bits(1, 2)
        .bits(8, 10)
        .bits(130, 10)
        .bits(1, 2) // one distance symbol, 42
        .bits(0, 2)
        .bits(42, 6)
        .bits(1, 1)
        .bits(2052, 14)
        .bits(0, 1)
        .done();
    assert_eq!(decompress(&s, 100).unwrap(), b"a");
    // the same empty command many times before the byte
    let mut w = block_start(1).bits(1, 2).bits(1, 2).bits(8, 10).bits(130, 10).bits(1, 2).bits(0, 2).bits(42, 6);
    for _ in 0..1000 {
        w = w.bits(1, 1).bits(2052, 14);
    }
    assert_eq!(decompress(&w.bits(0, 1).done(), 100).unwrap(), b"a");
}

#[test]
fn each_broken_rule_is_an_error() {
    let header = || W::default().bits(0, 1).bits(0, 1);
    // (the error, the stream)
    let cases: [(&str, Vec<u8>); 11] = [
        ("large window streams are not supported", W::default().bits(1, 1).bits(0, 3).bits(1, 3).done()),
        ("reserved bit set", header().bits(3, 2).bits(1, 1).done()),
        ("nonzero padding at the end", W::default().bits(0, 1).bits(1, 1).bits(1, 1).bits(1, 1).done()),
        (
            "meta-block length has a zero last nibble",
            header().bits(1, 2).bits(0x0ffff, 20).bits(0, 1).bits(0, 8).done(),
        ),
        ("metadata length has a zero last byte", header().bits(3, 2).bits(0, 1).bits(2, 2).bits(0xff, 16).done()),
        (
            "nonzero padding before uncompressed data",
            header().bits(0, 2).bits(0, 16).bits(1, 1).bits(1, 3).bits(0, 8).done(),
        ),
        ("dictionary transform out of range", last_block(4, 128 | 2, 16 + 29).bits(25604, 15).done()),
        ("dictionary word past the end of the meta-block", last_block(3, 128 | 2, 16).bits(0, 1).done()),
        ("copy past the end of the meta-block", last_block(5, 128 | 2 << 3 | 2, 16).bits(0, 1).done()),
        ("dictionary word length out of range", last_block(4, 128, 16).bits(0, 1).done()),
        ("insert past the end of the meta-block", last_block(2, 3 << 3, 16).done()),
    ];
    for (why, s) in cases {
        assert_eq!(decompress(&s, 1 << 20), Err(Error::Format(why)));
    }
}

#[test]
fn bad_prefix_codes_are_errors() {
    let start = || {
        W::default().bits(0, 1).bits(1, 1).bits(0, 1).bits(0, 2).bits(0, 16).bits(0, 3).bits(0, 6).bits(0, 2).bits(0, 2)
    };
    // two literal symbols, both 'a'
    let dup = start().bits(1, 2).bits(1, 2).bits(0x61, 8).bits(0x61, 8).done();
    assert!(format_error(&dup));
    // a command symbol past the 704-symbol alphabet
    let past = start().bits(1, 2).bits(0, 2).bits(0x61, 8).bits(1, 2).bits(0, 2).bits(1000, 10).done();
    assert!(format_error(&past));
    // code length code lengths 2 and 2 and nothing else: an incomplete code
    let mut w = start().bits(0, 2).bits(0b011, 3).bits(0b011, 3);
    for _ in 0..16 {
        w = w.bits(0, 2);
    }
    assert!(format_error(&w.done()));
}

#[test]
fn every_flipped_bit_ends_without_a_panic() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let mut errors = 0;
    let mut runs = 0;
    for name in ["monkey.compressed", "quickfox_repeated.compressed", "x.compressed.03", "ukkonooa.compressed"] {
        let base = std::fs::read(dir.join(name)).unwrap();
        for bit in 0..base.len() * 8 {
            let mut s = base.clone();
            s[bit / 8] ^= 1 << (bit % 8);
            runs += 1;
            errors += usize::from(decompress(&s, 1 << 20).is_err());
        }
    }
    // most flips break the stream; the rest decode to something else
    assert!(errors * 2 > runs, "{errors} errors in {runs} runs");
}
