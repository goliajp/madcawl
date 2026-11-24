use std::path::PathBuf;

pub fn data(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/ucd-18.0.0")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

pub fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap_or_else(|e| panic!("{s}: {e}"))
}

/// Prints the line the conformance stamp reads, then fails unless every case passed and the data held the
/// number of cases its version has.
pub fn report(suite: &str, pass: usize, total: usize, expected: usize, failures: &[String]) {
    for f in failures.iter().take(20) {
        println!("FAIL {f}");
    }
    println!("suite {suite} pass {pass} total {total}");
    assert_eq!(total, expected, "{suite}: cases read");
    assert_eq!(pass, total, "{suite}: {} cases failed", total - pass);
}
