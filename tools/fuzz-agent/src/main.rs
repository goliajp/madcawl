//! Mutation fuzzing of the agent protocol's request frames, the way
//! `madcawl-fuzz` drives the parsers: inputs from a seed corpus mutated
//! by a seeded generator, so a run is reproducible from its seed, each
//! input checked fed whole and in chunks (see `agent.rs`), a panic on any
//! thread counted as a crash.
//!
//!   madcawl-fuzz-agent agent-frames --seconds N [--seed S] [--out DIR]
//!
//! Prints one JSON line with the counts at the end, in the shape
//! `madcawl-fuzz` prints; every input that failed a check is written to
//! DIR with what it failed.

mod agent;
#[allow(dead_code)]
#[path = "../../fuzz/src/mutate.rs"]
mod mutate;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use wpt::{Feed, Rng};

/// Panics on any thread, counted by the hook `main` sets.
pub static PANICS: AtomicU64 = AtomicU64::new(0);

/// The generator and the ways of feeding `mutate.rs` and `agent.rs` use.
mod wpt {
    pub struct Rng(pub u64);

    impl Rng {
        pub fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
    }

    #[allow(dead_code)]
    #[derive(Clone, Copy, Debug)]
    pub enum Feed {
        Whole,
        Bytewise,
        Chunks { seed: u64, max: usize },
    }
}

struct Args {
    seconds: u64,
    seed: u64,
    out: PathBuf,
}

fn args() -> Args {
    let mut a = std::env::args().skip(1);
    let target = a.next().unwrap_or_default();
    assert_eq!(target, "agent-frames", "the one target is agent-frames");
    let mut r = Args { seconds: 60, seed: 1, out: PathBuf::from("fuzz-out") };
    while let Some(k) = a.next() {
        let v = a.next().unwrap_or_else(|| panic!("{k} needs a value"));
        match k.as_str() {
            "--seconds" => r.seconds = v.parse().expect("seconds"),
            "--seed" => r.seed = v.parse().expect("seed"),
            "--out" => r.out = PathBuf::from(v),
            _ => panic!("unknown option {k}"),
        }
    }
    r
}

/// The first few inputs of each kind are kept; past that only counted.
fn keep(a: &Args, kind: &str, n: u64, input: &[u8], why: &str) {
    if n > 20 {
        return;
    }
    let base = a.out.join(format!("agent-frames-{kind}-{n}"));
    std::fs::write(base.with_extension("bin"), input).unwrap();
    std::fs::write(base.with_extension("txt"), why).unwrap();
}

fn main() {
    let a = args();
    std::fs::create_dir_all(&a.out).unwrap();
    std::panic::set_hook(Box::new(|_| {
        PANICS.fetch_add(1, Ordering::SeqCst);
    }));
    let seeds = agent::corpus();
    let mut rng = Rng(a.seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let (mut execs, mut crashes, mut mismatches, mut max_len) = (0u64, 0u64, 0u64, 0usize);
    let started = Instant::now();
    let end = started + Duration::from_secs(a.seconds);
    while Instant::now() < end {
        let input = mutate::input(&seeds, agent::PIECES, &mut rng);
        let chunks = Feed::Chunks { seed: rng.next(), max: 1 + (rng.next() % 64) as usize };
        execs += 1;
        max_len = max_len.max(input.len());
        match catch_unwind(AssertUnwindSafe(|| (agent::run(&input, Feed::Whole), agent::run(&input, chunks)))) {
            Ok((whole, chunked)) if whole != chunked => {
                mismatches += 1;
                keep(&a, "mismatch", mismatches, &input, &format!("{chunks:?}"));
            }
            Ok(_) => {}
            Err(e) => {
                crashes += 1;
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                keep(&a, "crash", crashes, &input, &msg);
            }
        }
    }
    agent::cleanup();
    println!(
        "{{\"target\":\"agent-frames\",\"seed\":{},\"seconds\":{},\"execs\":{execs},\"crashes\":{crashes},\
         \"mismatches\":{mismatches},\"slow\":0,\"audited\":0,\"auditFailures\":0,\"auditMismatches\":0,\
         \"maxStepsPerByte\":0.0,\"stepsPerByteLimit\":0,\"corpus\":{},\"maxLen\":{max_len}}}",
        a.seed,
        started.elapsed().as_secs(),
        seeds.len()
    );
}
