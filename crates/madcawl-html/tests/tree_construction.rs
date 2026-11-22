//! The WPT tree construction tests (html5lib format), every one of
//! them, run with the whole input at once, fed one byte at a time, and
//! fed in chunks of random size. The tree is built in the real DOM,
//! whose counts are audited after every run.

#[path = "support/wpt.rs"]
mod wpt;

use wpt::{Feed, parse_tests, run};

#[test]
fn wpt_tree_construction_tests() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/wpt-tree-construction");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "dat"))
        .collect();
    files.sort();
    let skip_list = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expect/wpt-parsing-skip.txt"),
    )
    .unwrap();
    let mut to_skip: std::collections::BTreeSet<String> = skip_list
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| l.split_whitespace().next().unwrap().to_string())
        .collect();
    let (mut tests_total, mut skipped) = (0, 0);
    let mut passed = [0usize; 3];
    let mut failures = Vec::new();
    let mut audit = madcawl_dom::Audit::default();
    let mut audited_runs = 0usize;
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let (mut f_total, mut f_skip) = (0, 0);
        let mut f_passed = [0usize; 3];
        for test in parse_tests(&std::fs::read(path).unwrap()) {
            if to_skip.remove(&format!("{name}:{}", test.line)) {
                skipped += 1;
                f_skip += 1;
                continue;
            }
            tests_total += 1;
            f_total += 1;
            let modes: &[bool] = match test.script {
                Some(s) => {
                    if s {
                        &[true]
                    } else {
                        &[false]
                    }
                }
                None => &[false, true],
            };
            let feeds = [
                Feed::Whole,
                Feed::Bytewise,
                Feed::Chunks {
                    seed: 0x6d61_6463_6177_6c00 ^ ((test.line as u64) << 8),
                    max: 17,
                },
            ];
            for (i, feed) in feeds.into_iter().enumerate() {
                let mut ok = true;
                for &scripting in modes {
                    let (got, a) = run(&test, scripting, feed);
                    audit.add(a);
                    audited_runs += 1;
                    if got != test.document {
                        ok = false;
                        failures.push(format!(
                            "{name}:{} scripting={scripting} feed={feed:?}{}\n#data\n{}\n#want\n{}\n#got\n{}",
                            test.line,
                            test.fragment.as_ref().map(|f| format!(" fragment={}", String::from_utf8_lossy(f))).unwrap_or_default(),
                            String::from_utf8_lossy(&test.data),
                            String::from_utf8_lossy(&test.document),
                            String::from_utf8_lossy(&got),
                        ));
                        break;
                    }
                }
                if ok {
                    passed[i] += 1;
                    f_passed[i] += 1;
                }
            }
        }
        eprintln!(
            "file {name} whole {}/{f_total} bytewise {}/{f_total} chunked {}/{f_total} skipped {f_skip}",
            f_passed[0], f_passed[1], f_passed[2]
        );
    }
    assert!(
        to_skip.is_empty(),
        "listed as skipped but not in the data: {to_skip:?}"
    );
    let shown: usize = std::env::var("TREE_FAILURES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    for f in failures.iter().take(shown) {
        eprintln!("{f}\n");
    }
    eprintln!(
        "{}/{tests_total} passed whole, {}/{tests_total} byte at a time, \
         {}/{tests_total} in random chunks, {skipped} skipped as listed",
        passed[0], passed[1], passed[2]
    );
    eprintln!(
        "dom-audit: {} nodes in {audited_runs} documents, {} mismatches \
         ({} counts, {} links, {} flags)",
        audit.nodes,
        audit.mismatches(),
        audit.counts,
        audit.links,
        audit.flags
    );
    assert!(failures.is_empty(), "{} runs failed", failures.len());
    assert_eq!(audit.mismatches(), 0, "the count audit found mismatches");
}
