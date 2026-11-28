//! Inputs built to make a parser do quadratic work, each parsed at two
//! sizes, N and 4N. Linear work costs about four times as much at 4N,
//! quadratic about sixteen; the test asks for less than six. The cost is
//! the instructions this thread retires, which the kernel counts per
//! thread: unlike time, the count does not move with the load on the
//! machine, which core the thread ran on or how much of the input fits
//! in the cache. It is the fewest of three parses at each size, since
//! the first one also warms up the allocator and the atom table. The
//! atom table is one for the process, and its size changes what a name
//! costs, so the tests here measure one at a time.

use std::sync::{Mutex, PoisonError};

use madcawl_dom::Dom;
use madcawl_html::{Options, Step, Tokenizer, TreeBuilder};

fn parse(input: &[u8]) -> Dom {
    let mut tok = Tokenizer::new();
    let mut builder = TreeBuilder::new(Dom::new(), Options::default());
    tok.feed(input);
    tok.end();
    assert_eq!(tok.run(&mut builder), Step::Done);
    builder.into_sink()
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn thread_selfcounts(kind: i32, counts: *mut u64, len: usize) -> i32;
}

/// Instructions this thread has retired.
#[cfg(target_os = "macos")]
fn instructions() -> u64 {
    // kind 1 is the pair of instructions and cycles
    let mut counts = [0u64; 2];
    // SAFETY: the buffer holds the two u64 counters of kind 1
    let r = unsafe { thread_selfcounts(1, counts.as_mut_ptr(), std::mem::size_of_val(&counts)) };
    assert_eq!(r, 0, "thread_selfcounts");
    counts[0]
}

/// Nanoseconds since a fixed start, where the thread's counters are not read.
#[cfg(not(target_os = "macos"))]
fn instructions() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_nanos() as u64
}

fn fewest(input: &[u8]) -> u64 {
    (0..3)
        .map(|_| {
            let before = instructions();
            std::hint::black_box(parse(input));
            instructions() - before
        })
        .min()
        .unwrap()
}

static MEASURING: Mutex<()> = Mutex::new(());

/// The ratio of the instructions at 4N to those at N.
fn ratio(name: &str, n: usize, make: impl Fn(usize) -> Vec<u8>) -> f64 {
    let (small, large) = (make(n), make(4 * n));
    // a test that failed while measuring leaves nothing half done
    let _one = MEASURING.lock().unwrap_or_else(PoisonError::into_inner);
    let (a, b) = (fewest(&small), fewest(&large));
    let r = b as f64 / a as f64;
    println!(
        "{name}: N={n} {a} instructions, 4N={} {b}, 4N/N {r:.2}",
        4 * n
    );
    r
}

const LIMIT: f64 = 6.0;

#[test]
fn deep_nesting() {
    let r = ratio("deep nesting", 20_000, |n| b"<div>".repeat(n));
    assert!(r < LIMIT, "deep nesting: 4N/N = {r:.2}");
}

#[test]
fn deep_nesting_stops_at_the_depth_limit() {
    let dom = parse(&b"<div>".repeat(2000));
    let depth = |mut n| {
        let mut d = 0;
        while let Some(p) = dom.parent(n) {
            n = p;
            d += 1;
        }
        d
    };
    let deepest = dom.descendants(dom.document()).map(depth).max().unwrap();
    // as Blink: nesting stops once 513 elements are open (html and 512
    // more), so the deepest element is 514 below the document
    assert_eq!(deepest, 514, "levels below the document");
    let mut out = Vec::new();
    dom.write_dat(dom.document(), &mut out);
    assert!(out.len() > 2000 * 5, "every div is still in the tree");
}

#[test]
fn many_attributes() {
    let r = ratio("many attributes", 5_000, |n| {
        let mut s = b"<a".to_vec();
        for i in 0..n {
            s.extend_from_slice(format!(" a{i}=x").as_bytes());
        }
        s.push(b'>');
        s
    });
    assert!(r < LIMIT, "many attributes: 4N/N = {r:.2}");
}

#[test]
fn many_formatting_elements() {
    let r = ratio("many formatting elements", 5_000, |n| {
        (0..n)
            .flat_map(|i| format!("<b x={i}>").into_bytes())
            .collect()
    });
    assert!(r < LIMIT, "many formatting elements: 4N/N = {r:.2}");
}

#[test]
fn many_tag_names() {
    // every name is new, so each start tag adds one to the atom table:
    // adding must not cost more as the table grows
    let r = ratio("many tag names", 20_000, |n| {
        (0..n)
            .flat_map(|i| format!("<t{i}></t{i}>").into_bytes())
            .collect()
    });
    assert!(r < LIMIT, "many tag names: 4N/N = {r:.2}");
}

#[test]
fn text_between_stray_end_tags() {
    // each run of text merges into the same text node
    let r = ratio("text between stray end tags", 20_000, |n| {
        b"ab</x>".repeat(n)
    });
    assert!(r < LIMIT, "text between stray end tags: 4N/N = {r:.2}");
}

#[test]
fn text_fostered_out_of_a_table() {
    // each run of table text is moved in front of the table, into the
    // text node already there
    let r = ratio("text fostered out of a table", 10_000, |n| {
        let mut s = b"<table>".to_vec();
        s.extend_from_slice(&b"ab<tr>".repeat(n));
        s
    });
    assert!(r < LIMIT, "text fostered out of a table: 4N/N = {r:.2}");
}

#[test]
fn many_selected_options() {
    // each selected option that finishes looks for the select's
    // selectedcontent; there is none, and finding that must not scan
    let r = ratio("many selected options", 1_000, |n| {
        let mut s = b"<select>".to_vec();
        s.extend_from_slice(&b"<option selected>x".repeat(n));
        s
    });
    assert!(r < LIMIT, "many selected options: 4N/N = {r:.2}");
}

#[test]
fn many_options_after_many_elements() {
    // whether an option is the first one must be known without a scan
    let r = ratio("many options after many elements", 1_000, |n| {
        let mut s = b"<select>".to_vec();
        s.extend_from_slice(&b"<div></div>".repeat(n));
        s.extend_from_slice(&b"<option></option>".repeat(n));
        s
    });
    assert!(r < LIMIT, "many options after many elements: 4N/N = {r:.2}");
}
