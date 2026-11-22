//! css-parsing-tests (tests/data/css-parsing-tests, pinned by REVISION):
//! every case not in tests/expect/css-parsing-skip.txt, which names
//! whole files or single cases (`file#n`, numbered from 1). A listed
//! case still runs, and passing fails the test: the list stays exact.
//! Prints a line per file and a summary line:
//!
//!   file <name> cases <n> passed <p> skipped <s>
//!   css-parsing: <pass>/<total> passed, <skip> skipped, <fail> failed

mod support;

use std::path::Path;

use madcawl_css::{
    Encoding, Span, decode_stylesheet, parse_anb, parse_block_contents, parse_component_value,
    parse_declaration, parse_declaration_list, parse_rule, parse_rule_list,
};
use support::json::{self, Json};
use support::to_json::{component, item, items, list, rule, syntax_error};

fn text(input: &Json) -> String {
    String::from_utf8(input.str().expect("a string input").to_vec()).expect("utf-8 input")
}

fn whole(src: &str) -> Span {
    Span::new(0, src.len())
}

fn stylesheet_bytes(input: &Json) -> Json {
    let latin1: Vec<u8> = text(input.get("css_bytes").unwrap())
        .chars()
        .map(|c| c as u32 as u8)
        .collect();
    let label = |k: &str| input.get(k).and_then(Json::str).map(|v| v.to_vec());
    let protocol = label("protocol_encoding");
    let environment = label("environment_encoding").and_then(|l| Encoding::for_label(&l));
    let (src, encoding) = decode_stylesheet(&latin1, protocol.as_deref(), environment);
    let rules = items(&parse_rule_list(&src, true), &src);
    Json::Arr(vec![
        rules,
        Json::Str(encoding.name().to_ascii_lowercase().into_bytes()),
    ])
}

fn run(file: &str, input: &Json) -> Json {
    if file == "stylesheet_bytes.json" {
        return stylesheet_bytes(input);
    }
    let src = text(input);
    let src = src.as_str();
    match file {
        "component_value_list.json" => list(src, whole(src)),
        "one_component_value.json" => match parse_component_value(src) {
            Ok(v) => {
                let mut out = Vec::new();
                component(&v, src, &mut out);
                out.remove(0)
            }
            Err(e) => syntax_error(e),
        },
        "declaration_list.json" => items(&parse_declaration_list(src), src),
        "blocks_contents.json" => items(&parse_block_contents(src, whole(src)), src),
        "one_declaration.json" => match parse_declaration(src) {
            Ok(d) => item(&madcawl_css::BlockItem::Declaration(d), src),
            Err(e) => syntax_error(e),
        },
        "one_rule.json" => match parse_rule(src) {
            Ok(r) => rule(&r, src),
            Err(e) => syntax_error(e),
        },
        "rule_list.json" => items(&parse_rule_list(src, false), src),
        "stylesheet.json" => items(&parse_rule_list(src, true), src),
        "An+B.json" => match parse_anb(src, whole(src)) {
            Some((a, b)) => Json::Arr(vec![Json::Num(a as f64), Json::Num(b as f64)]),
            None => Json::Null,
        },
        _ => panic!("no runner for {file}"),
    }
}

#[test]
fn css_parsing_tests() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/css-parsing-tests");
    let skip_list = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expect/css-parsing-skip.txt"),
    )
    .unwrap();
    let entries: Vec<&str> = skip_list
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    let skipped_files: Vec<&str> = entries
        .iter()
        .copied()
        .filter(|e| !e.contains('#'))
        .collect();
    let skipped_cases: Vec<(&str, usize)> = entries
        .iter()
        .filter_map(|e| e.split_once('#'))
        .map(|(f, n)| (f, n.parse().unwrap()))
        .collect();
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".json"))
        .collect();
    files.sort();
    for s in skipped_files
        .iter()
        .chain(skipped_cases.iter().map(|c| &c.0))
    {
        assert!(
            files.iter().any(|f| f == s),
            "skip list names {s}, not in the suite"
        );
    }
    let (mut total, mut passed, mut skipped, mut shown) = (0, 0, 0, 0);
    let mut stale = Vec::new();
    for file in &files {
        let cases = json::parse(&std::fs::read_to_string(dir.join(file)).unwrap());
        let cases = cases.arr();
        assert_eq!(cases.len() % 2, 0, "{file}: odd number of items");
        let n = cases.len() / 2;
        total += n;
        if skipped_files.contains(&file.as_str()) {
            skipped += n;
            eprintln!("file {file} cases {n} passed 0 skipped {n}");
            continue;
        }
        let (mut p, mut sk) = (0, 0);
        for (i, pair) in cases.chunks(2).enumerate() {
            let got = run(file, &pair[0]);
            if skipped_cases.contains(&(file.as_str(), i + 1)) {
                sk += 1;
                if got == pair[1] {
                    stale.push(format!("{file}#{}", i + 1));
                }
            } else if got == pair[1] {
                p += 1;
            } else if shown < 20 {
                shown += 1;
                eprintln!(
                    "FAIL {file}\n  input {:?}\n  want  {:?}\n  got   {:?}",
                    pair[0], pair[1], got
                );
            }
        }
        passed += p;
        skipped += sk;
        eprintln!("file {file} cases {n} passed {p} skipped {sk}");
    }
    let failed = total - passed - skipped;
    eprintln!("css-parsing: {passed}/{total} passed, {skipped} skipped, {failed} failed");
    assert_eq!(failed, 0, "{failed} cases failed");
    assert!(stale.is_empty(), "skipped cases that pass: {stale:?}");
}
