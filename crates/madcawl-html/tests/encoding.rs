//! The html5lib encoding tests, each parsed as a whole document with
//! the whole input at once, fed one byte at a time and fed in random
//! chunks; then the decoders and the start-over a late `meta` causes.

#[path = "support/dom.rs"]
mod dom;

use std::path::Path;

use dom::{Dom, print};
use madcawl_html::encoding::prescan;
use madcawl_html::{Confidence, Decoder, DocumentParser, Encoding, Options, Source, Step};

#[derive(Clone, Copy, Debug)]
enum Feed {
    Whole,
    Bytewise,
    Chunks(u64),
}

/// xorshift64*: deterministic, no dependency
fn next(x: &mut u64) -> u64 {
    *x ^= *x >> 12;
    *x ^= *x << 25;
    *x ^= *x >> 27;
    x.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

fn parse(data: &[u8], source: Source, feed: Feed) -> (Encoding, Confidence, Dom) {
    let mut p = DocumentParser::new(Dom::new, Options::default(), source);
    match feed {
        Feed::Whole => p.feed(data),
        Feed::Bytewise => {
            for b in data {
                p.feed(std::slice::from_ref(b));
                assert_eq!(p.run(), Step::NeedInput);
            }
        }
        Feed::Chunks(seed) => {
            let mut rng = seed | 1;
            let mut rest = data;
            while !rest.is_empty() {
                let n = (1 + next(&mut rng) as usize % 17).min(rest.len());
                p.feed(&rest[..n]);
                assert_eq!(p.run(), Step::NeedInput);
                rest = &rest[n..];
            }
        }
    }
    p.end();
    assert_eq!(p.run(), Step::Done);
    (p.encoding(), p.confidence(), p.into_sink())
}

/// `(line of #data, data, expected encoding)` for each test of a file.
fn cases(text: &[u8]) -> Vec<(usize, Vec<u8>, Vec<u8>)> {
    let lines: Vec<&[u8]> = text.split(|&b| b == b'\n').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i] != b"#data" {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while lines[end] != b"#encoding" {
            end += 1;
        }
        out.push((
            start,
            lines[start..end].join(&b'\n'),
            lines[end + 1].to_vec(),
        ));
        i = end + 2;
    }
    out
}

#[test]
fn html5lib_encoding_tests() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/html5lib-encoding");
    let skip_list = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expect/html5lib-encoding-skip.txt"),
    )
    .unwrap();
    let mut to_skip: std::collections::BTreeSet<String> = skip_list
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| l.split_whitespace().next().unwrap().to_string())
        .collect();
    let files = [
        "tests1.dat",
        "tests2.dat",
        "test-yahoo-jp.dat",
        "scripted/tests1.dat",
    ];
    let (mut total, mut skipped) = (0, 0);
    let mut passed = [0usize; 3];
    let mut failures = Vec::new();
    for name in files {
        for (line, data, want) in cases(&std::fs::read(dir.join(name)).unwrap()) {
            if to_skip.remove(&format!("{name}:{line}")) {
                skipped += 1;
                continue;
            }
            total += 1;
            let feeds = [
                Feed::Whole,
                Feed::Bytewise,
                Feed::Chunks(0x0065_6e63 ^ line as u64),
            ];
            for (i, feed) in feeds.into_iter().enumerate() {
                let (got, _, _) = parse(&data, Source::default(), feed);
                if got.name().as_bytes().eq_ignore_ascii_case(&want) {
                    passed[i] += 1;
                } else {
                    failures.push(format!(
                        "{name}:{line} feed={feed:?} want {} got {}",
                        String::from_utf8_lossy(&want),
                        got.name()
                    ));
                }
            }
        }
    }
    assert!(
        to_skip.is_empty(),
        "listed as skipped but not in the data: {to_skip:?}"
    );
    for f in &failures {
        eprintln!("{f}");
    }
    eprintln!(
        "{}/{total} passed whole, {}/{total} byte at a time, \
         {}/{total} in random chunks, {skipped} skipped as listed",
        passed[0], passed[1], passed[2]
    );
    assert!(failures.is_empty(), "{} runs failed", failures.len());
}

fn utf8(input: &[u8]) -> String {
    let mut out = Vec::new();
    Decoder::new(Encoding::Utf8).decode(input, &mut out, true);
    let whole = String::from_utf8(out).unwrap();
    let mut out = Vec::new();
    let mut d = Decoder::new(Encoding::Utf8);
    for b in input {
        d.decode(std::slice::from_ref(b), &mut out, false);
    }
    d.decode(&[], &mut out, true);
    assert_eq!(whole, String::from_utf8(out).unwrap(), "byte at a time");
    whole
}

#[test]
fn utf8_errors_are_maximal_subparts() {
    // a cut-off sequence is one error and the byte that broke it is read again
    assert_eq!(utf8(b"\xE2\x82\x41"), "\u{FFFD}A");
    // a surrogate's encoding is not UTF-8: every byte is its own error
    assert_eq!(utf8(b"\xED\xA0\x80"), "\u{FFFD}\u{FFFD}\u{FFFD}");
    assert_eq!(utf8(b"\xF0\x80\x80"), "\u{FFFD}\u{FFFD}\u{FFFD}");
    assert_eq!(
        utf8(b"\xF4\x90\x80\x80"),
        "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}"
    );
    assert_eq!(utf8(b"\xC0\xAF"), "\u{FFFD}\u{FFFD}");
    assert_eq!(utf8(b"\xF1\x80\x80"), "\u{FFFD}");
    assert_eq!(utf8(b"a\xF1\x80\x80"), "a\u{FFFD}");
    // the Unicode Standard's example of the practice (table 3-8)
    assert_eq!(
        utf8(b"\x61\xF1\x80\x80\xE1\x80\xC2\x62\x80\x63\x80\xBF\x64"),
        "a\u{FFFD}\u{FFFD}\u{FFFD}b\u{FFFD}c\u{FFFD}\u{FFFD}d"
    );
    assert_eq!(utf8("é😀".as_bytes()), "é😀");
}

#[test]
fn utf16_decodes_pairs_and_lone_halves() {
    let decode = |e, input: &[u8]| {
        let mut out = Vec::new();
        Decoder::new(e).decode(input, &mut out, true);
        String::from_utf8(out).unwrap()
    };
    assert_eq!(decode(Encoding::Utf16le, b"A\0\x3D\xD8\x00\xDE"), "A😀");
    assert_eq!(decode(Encoding::Utf16be, b"\0A\xD8\x3D\xDE\x00"), "A😀");
    assert_eq!(decode(Encoding::Utf16le, b"\x3D\xD8A\0"), "\u{FFFD}A");
    assert_eq!(decode(Encoding::Utf16le, b"A\0B"), "A\u{FFFD}");
}

fn tree(dom: &Dom) -> String {
    let mut out = Vec::new();
    print(dom, 0, 0, &mut out);
    String::from_utf8(out).unwrap()
}

#[test]
fn a_late_meta_starts_the_parse_over_in_its_encoding() {
    let mut doc = b"<!--".to_vec();
    doc.extend(std::iter::repeat_n(b'x', 2000));
    doc.extend_from_slice("--><meta charset=utf-8><p>ñ</p>".as_bytes());
    for feed in [Feed::Whole, Feed::Bytewise, Feed::Chunks(7)] {
        let (e, c, dom) = parse(&doc, Source::default(), feed);
        assert_eq!((e, c), (Encoding::Utf8, Confidence::Certain));
        assert!(tree(&dom).contains("\"ñ\""), "{feed:?}");
    }
}

#[test]
fn a_bom_and_the_transport_layer_are_certain() {
    let transport = Source {
        transport: Some(Encoding::Utf8),
        ..Source::default()
    };
    let (e, c, _) = parse(b"<meta charset=iso-8859-2>", transport, Feed::Whole);
    assert_eq!((e, c), (Encoding::Utf8, Confidence::Certain));
    let (e, c, dom) = parse(b"\xEF\xBB\xBF<p>x", transport, Feed::Bytewise);
    assert_eq!((e, c), (Encoding::Utf8, Confidence::Certain));
    assert!(!tree(&dom).contains('\u{FEFF}'));
    let (e, _, dom) = parse(b"\xFF\xFEa\0", Source::default(), Feed::Whole);
    assert_eq!(e, Encoding::Utf16le);
    assert!(tree(&dom).contains("\"a\""));
    let late = format!("<!--{}--><meta charset=iso-8859-2>", "x".repeat(2000));
    let (e, c, _) = parse(late.as_bytes(), transport, Feed::Whole);
    assert_eq!((e, c), (Encoding::Utf8, Confidence::Certain));
}

#[test]
fn prescan_names_utf16_xml_declarations_and_reads_no_further_than_it_has() {
    assert_eq!(prescan(b"<\0?\0x\0m\0l\0"), Some(Encoding::Utf16le));
    assert_eq!(
        prescan(b"<meta charset=\"x-user-defined\">"),
        Some(Encoding::Windows1252)
    );
    assert_eq!(prescan(b"<meta charset=utf-8"), None);
    assert_eq!(
        Encoding::for_label(b" Latin1\n"),
        Some(Encoding::Windows1252)
    );
    assert_eq!(Encoding::for_label(b"bogus"), None);
}

struct Chars(Vec<u8>);

impl madcawl_html::Sink for Chars {
    fn token(&mut self, t: madcawl_html::Token<'_>) -> madcawl_html::Flow {
        if let madcawl_html::Token::Chars(s) = t {
            self.0.extend_from_slice(s);
        }
        madcawl_html::Flow::Continue
    }
}

fn tokenize_raw(bytes: &[u8]) -> Vec<u8> {
    let mut t = madcawl_html::Tokenizer::new();
    let mut sink = Chars(Vec::new());
    t.feed(bytes);
    t.end();
    t.run(&mut sink);
    sink.0
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "not WTF-8")]
fn a_debug_build_stops_on_tokenizer_input_that_is_not_wtf8() {
    tokenize_raw(b"a\xE2\x82\x41");
}

#[cfg(not(debug_assertions))]
#[test]
fn a_release_build_reads_tokenizer_input_that_is_not_wtf8_as_replacements() {
    assert_eq!(
        tokenize_raw(b"a\xE2\x82\x41"),
        "a\u{FFFD}\u{FFFD}A".as_bytes()
    );
}
