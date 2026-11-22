//! Mutation fuzzing of the tokenizer and the tree builder, with no
//! fuzzing library: inputs are the tree construction tests' inputs,
//! mutated by a seeded generator, so a run is reproducible from its
//! seed. Each input is checked three ways: the parser does not panic;
//! fed in random chunks it gives what it gives fed whole; its work
//! (`madcawl_html::steps`) stays under a fixed multiple of its length.
//! The tree target also audits the counts and links of both DOMs it
//! built (`madcawl_dom::Dom::audit`).
//!
//!   madcawl-fuzz <tokenizer|tree> --seconds N [--seed S] [--out DIR] [--steps-per-byte K]
//!
//! Prints one JSON line with the counts at the end; every input that
//! failed a check is written to DIR with what it failed.

mod mutate;
mod targets;
#[allow(dead_code)]
#[path = "../../../crates/madcawl-html/tests/support/wpt.rs"]
mod wpt;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use wpt::{Feed, Rng};

/// Work allowed per input byte (`--steps-per-byte` changes it), and for
/// the empty input. The parser's scans are bounded by the depth limit
/// (512) and the formatting list limit (256); the tree builder's most
/// per byte seen in the first runs was under 700.
const STEPS_PER_BYTE: u64 = 2048;
const STEPS_BASE: u64 = 1 << 16;

struct Args {
    target: String,
    seconds: u64,
    seed: u64,
    out: PathBuf,
    steps_per_byte: u64,
}

fn args() -> Args {
    let mut a = std::env::args().skip(1);
    let target = a.next().unwrap_or_default();
    let mut r = Args {
        target,
        seconds: 60,
        seed: 1,
        out: PathBuf::from("fuzz-out"),
        steps_per_byte: STEPS_PER_BYTE,
    };
    while let Some(k) = a.next() {
        let v = a.next().unwrap_or_else(|| panic!("{k} needs a value"));
        match k.as_str() {
            "--seconds" => r.seconds = v.parse().expect("seconds"),
            "--seed" => r.seed = v.parse().expect("seed"),
            "--out" => r.out = PathBuf::from(v),
            "--steps-per-byte" => r.steps_per_byte = v.parse().expect("steps per byte"),
            _ => panic!("unknown option {k}"),
        }
    }
    r
}

fn corpus() -> Vec<Vec<u8>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/madcawl-html/tests/data/wpt-tree-construction");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "dat"))
        .collect();
    files.sort();
    let mut out: Vec<Vec<u8>> = files
        .iter()
        .flat_map(|p| wpt::parse_tests(&std::fs::read(p).unwrap()))
        .map(|t| t.data)
        .collect();
    out.sort();
    out.dedup();
    out
}

#[derive(Default)]
struct Counts {
    execs: u64,
    crashes: u64,
    mismatches: u64,
    slow: u64,
    audited: u64,
    audit_failures: u64,
    audit_mismatches: u64,
    max_steps_per_byte: f64,
    max_len: usize,
}

fn main() {
    let a = args();
    let run: fn(&[u8], Feed) -> targets::Out = match a.target.as_str() {
        "tokenizer" => targets::tokenizer,
        "tree" => targets::tree,
        t => panic!("no target {t:?}: tokenizer or tree"),
    };
    std::fs::create_dir_all(&a.out).unwrap();
    std::panic::set_hook(Box::new(|_| {}));
    let seeds = corpus();
    let mut rng = Rng(a.seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut c = Counts::default();
    let end = Instant::now() + Duration::from_secs(a.seconds);
    let started = Instant::now();
    while Instant::now() < end {
        let input = mutate::input(&seeds, &mut rng);
        let chunks = Feed::Chunks {
            seed: rng.next(),
            max: 1 + (rng.next() % 64) as usize,
        };
        c.execs += 1;
        c.max_len = c.max_len.max(input.len());
        let got = catch_unwind(AssertUnwindSafe(|| {
            (run(&input, Feed::Whole), run(&input, chunks))
        }));
        let (whole, chunked) = match got {
            Ok(r) => r,
            Err(e) => {
                c.crashes += 1;
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                keep(&a, "crash", c.crashes, &input, &msg);
                continue;
            }
        };
        if whole.flat != chunked.flat {
            c.mismatches += 1;
            keep(&a, "mismatch", c.mismatches, &input, &format!("{chunks:?}"));
        }
        for audit in [whole.audit, chunked.audit].into_iter().flatten() {
            c.audited += 1;
            if audit.mismatches() != 0 {
                c.audit_failures += 1;
                c.audit_mismatches += audit.mismatches();
                keep(&a, "audit", c.audit_failures, &input, &format!("{audit:?}"));
            }
        }
        let steps = whole.steps.max(chunked.steps);
        let len = input.len() as u64;
        c.max_steps_per_byte = c.max_steps_per_byte.max(steps as f64 / (len + 1) as f64);
        if steps > STEPS_BASE + a.steps_per_byte * len {
            c.slow += 1;
            keep(
                &a,
                "slow",
                c.slow,
                &input,
                &format!("{steps} steps for {len} bytes"),
            );
        }
    }
    println!(
        "{{\"target\":\"{}\",\"seed\":{},\"seconds\":{},\"execs\":{},\"crashes\":{},\"mismatches\":{},\
         \"slow\":{},\"audited\":{},\"auditFailures\":{},\"auditMismatches\":{},\"maxStepsPerByte\":{:.1},\
         \"stepsPerByteLimit\":{},\"corpus\":{},\"maxLen\":{}}}",
        a.target,
        a.seed,
        started.elapsed().as_secs(),
        c.execs,
        c.crashes,
        c.mismatches,
        c.slow,
        c.audited,
        c.audit_failures,
        c.audit_mismatches,
        c.max_steps_per_byte,
        a.steps_per_byte,
        seeds.len(),
        c.max_len
    );
}

/// The first few inputs of each kind are kept; past that only counted.
fn keep(a: &Args, kind: &str, n: u64, input: &[u8], why: &str) {
    if n > 20 {
        return;
    }
    let base = a.out.join(format!("{}-{kind}-{n}", a.target));
    std::fs::write(base.with_extension("bin"), input).unwrap();
    std::fs::write(base.with_extension("txt"), why).unwrap();
}
