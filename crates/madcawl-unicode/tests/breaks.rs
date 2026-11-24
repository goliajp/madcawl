//! The UAX #14 and UAX #29 conformance data: LineBreakTest, GraphemeBreakTest, WordBreakTest.

mod support;

use madcawl_unicode::{LineBreaks, Utf32, grapheme_breaks, word_breaks};
use support::{data, hex, report};

// each case: its code points and the offsets after the start where the data marks a break
fn cases(file: &str) -> Vec<(usize, Vec<u32>, Vec<usize>)> {
    let mut out = Vec::new();
    for (n, line) in data(file).lines().enumerate() {
        let body = line.split('#').next().unwrap_or("").trim();
        if body.is_empty() {
            continue;
        }
        let (mut cps, mut brk) = (Vec::new(), Vec::new());
        for tok in body.split_whitespace() {
            match tok {
                "÷" if !cps.is_empty() => brk.push(cps.len()),
                "÷" | "×" => {}
                h => cps.push(hex(h)),
            }
        }
        out.push((n + 1, cps, brk));
    }
    out
}

fn run(suite: &str, file: &str, expected: usize, breaks: impl Fn(&[u32]) -> Vec<usize>) {
    let (mut pass, mut total, mut failures) = (0, 0, Vec::new());
    for (line, cps, want) in cases(file) {
        total += 1;
        let got = breaks(&cps);
        // the same text as UTF-8 must break at the same characters
        let utf8 = cps
            .iter()
            .map(|&c| char::from_u32(c))
            .collect::<Option<String>>();
        let same = utf8.is_none_or(|s| {
            let starts: Vec<usize> = s.char_indices().map(|(i, _)| i).chain([s.len()]).collect();
            let mapped: Vec<usize> = got.iter().map(|&k| starts[k]).collect();
            mapped == breaks_utf8(suite, &s)
        });
        if got == want && same {
            pass += 1;
        } else {
            failures.push(format!(
                "{file}:{line} want {want:?} got {got:?} utf8 agrees {same}"
            ));
        }
    }
    report(suite, pass, total, expected, &failures);
}

fn breaks_utf8(suite: &str, s: &str) -> Vec<usize> {
    match suite {
        "LineBreakTest" => LineBreaks::new(s).map(|b| b.pos).collect(),
        "GraphemeBreakTest" => grapheme_breaks(s).collect(),
        _ => word_breaks(s).collect(),
    }
}

#[test]
fn line_break_test() {
    run("LineBreakTest", "LineBreakTest.txt", 19_346, |c| {
        LineBreaks::new(&Utf32(c)).map(|b| b.pos).collect()
    });
}

#[test]
fn grapheme_break_test() {
    run("GraphemeBreakTest", "GraphemeBreakTest.txt", 853, |c| {
        grapheme_breaks(&Utf32(c)).collect()
    });
}

#[test]
fn word_break_test() {
    run("WordBreakTest", "WordBreakTest.txt", 1_944, |c| {
        word_breaks(&Utf32(c)).collect()
    });
}
