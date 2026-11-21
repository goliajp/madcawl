//! The WPT tree construction tests (html5lib format), every one of
//! them, run with the whole input at once and fed one byte at a time.

use madcawl_html::{
    Attrs, FragmentContext, Namespace, Options, QuirksMode, Step, Tokenizer, TreeBuilder, TreeSink,
};

#[path = "support/dom.rs"]
mod dom;

use dom::{Dom, print};

#[derive(Default)]
struct Test {
    data: Vec<u8>,
    fragment: Option<Vec<u8>>,
    script: Option<bool>,
    document: Vec<u8>,
    line: usize,
}

fn parse_tests(text: &[u8]) -> Vec<Test> {
    let mut tests = Vec::new();
    let mut section: &[u8] = b"";
    let mut prev_empty = true;
    let mut cur: Option<Test> = None;
    let mut lines: Vec<&[u8]> = Vec::new();
    let flush = |cur: &mut Option<Test>, section: &[u8], lines: &mut Vec<&[u8]>| {
        let Some(t) = cur else { return };
        match section {
            b"#data" => t.data = lines.join(&b'\n'),
            b"#document-fragment" => t.fragment = Some(lines.join(&b'\n')),
            b"#document" => {
                while lines.last().is_some_and(|l| l.is_empty()) {
                    lines.pop();
                }
                t.document = lines.join(&b'\n');
            }
            _ => {}
        }
        lines.clear();
    };
    for (n, line) in text.split(|&b| b == b'\n').enumerate() {
        let header = line.starts_with(b"#") && section != b"#document" || line == b"#data";
        if matches!(line, b"#data" | b"#description") && prev_empty {
            flush(&mut cur, section, &mut lines);
            tests.extend(cur.take());
            cur = Some(Test {
                line: n + 1,
                ..Test::default()
            });
            section = line;
        } else if line == b"#data" && section == b"#description" {
            lines.clear();
            section = line;
        } else if header
            && matches!(
                line,
                b"#errors"
                    | b"#new-errors"
                    | b"#document-fragment"
                    | b"#script-on"
                    | b"#script-off"
                    | b"#document"
                    | b"#description"
            )
        {
            flush(&mut cur, section, &mut lines);
            section = line;
            if let Some(t) = cur.as_mut() {
                match line {
                    b"#script-on" => t.script = Some(true),
                    b"#script-off" => t.script = Some(false),
                    _ => {}
                }
            }
        } else {
            lines.push(line);
        }
        prev_empty = line.is_empty();
    }
    flush(&mut cur, section, &mut lines);
    tests.extend(cur);
    tests
}

fn run(test: &Test, scripting: bool, bytewise: bool) -> Vec<u8> {
    let opts = Options {
        scripting,
        ..Options::default()
    };
    let mut dom = Dom::new();
    let mut tok = Tokenizer::new();
    let mut root = 0;
    let mut builder = match &test.fragment {
        None => TreeBuilder::new(dom, opts),
        Some(ctx) => {
            let (ns, local) = match ctx.split(|&b| b == b' ').collect::<Vec<_>>()[..] {
                [b"svg", l] => (Namespace::Svg, l),
                [b"math", l] => (Namespace::MathMl, l),
                [l] => (Namespace::Html, l),
                _ => panic!("bad fragment context"),
            };
            let form = (ns == Namespace::Html && local == b"form")
                .then(|| dom.create_element(Namespace::Html, b"form", Attrs::empty()));
            root = dom.nodes.len();
            let b = TreeBuilder::new_fragment(
                dom,
                opts,
                FragmentContext {
                    ns,
                    local,
                    attrs: &[],
                    form,
                    quirks: QuirksMode::NoQuirks,
                },
            );
            tok.set_state(b.tokenizer_state());
            b
        }
    };
    if bytewise {
        for b in &test.data {
            tok.feed(std::slice::from_ref(b));
            assert_eq!(tok.run(&mut builder), Step::NeedInput);
        }
    } else {
        tok.feed(&test.data);
    }
    tok.end();
    assert_eq!(tok.run(&mut builder), Step::Done);
    let dom = builder.into_sink();
    let mut out = Vec::new();
    print(&dom, root, 0, &mut out);
    out.pop();
    out
}

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
    let (mut tests_total, mut passed, mut bytewise_passed, mut skipped) = (0, 0, 0, 0);
    let mut failures = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let (mut f_total, mut f_whole, mut f_byte, mut f_skip) = (0, 0, 0, 0);
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
            for bytewise in [false, true] {
                let mut ok = true;
                for &scripting in modes {
                    let got = run(&test, scripting, bytewise);
                    if got != test.document {
                        ok = false;
                        failures.push(format!(
                            "{name}:{} scripting={scripting} bytewise={bytewise}{}\n#data\n{}\n#want\n{}\n#got\n{}",
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
                    if bytewise {
                        bytewise_passed += 1;
                        f_byte += 1;
                    } else {
                        passed += 1;
                        f_whole += 1;
                    }
                }
            }
        }
        eprintln!("file {name} whole {f_whole}/{f_total} bytewise {f_byte}/{f_total} skipped {f_skip}");
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
        "{passed}/{tests_total} passed whole, {bytewise_passed}/{tests_total} byte at a time, \
         {skipped} skipped as listed"
    );
    assert!(failures.is_empty(), "{} runs failed", failures.len());
}
