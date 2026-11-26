//! The tree construction cases whose input runs script (WPT
//! `html/syntax/parsing/resources/scripted_*.dat`, kept with the other
//! tree construction data in madcawl-html): the parser stops at each
//! script's end tag, the script runs on JavaScriptCore, and
//! `document.write` feeds the parser.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js_jsc::Jsc;

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../madcawl-html/tests/data/wpt-tree-construction");
const FILES: [&str; 4] = ["scripted_adoption01.dat", "scripted_ark.dat", "scripted_foster01.dat", "scripted_webkit01.dat"];

struct Case {
    file: &'static str,
    data: String,
    document: String,
}

fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    for file in FILES {
        let text = std::fs::read_to_string(format!("{DATA}/{file}")).expect("the data file");
        for chunk in text.split("#data\n").skip(1) {
            let data = chunk.split("\n#errors").next().expect("a data section").to_owned();
            let doc = chunk.split("#document\n").nth(1).expect("a document section");
            out.push(Case { file, data, document: doc.trim_end_matches('\n').to_owned() });
        }
    }
    out
}

fn run(case: &Case) -> (String, u32) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    h.parse_document(&mut e, case.data.as_bytes());
    let mut out = Vec::new();
    h.dom.write_dat(h.dom.document(), &mut out);
    let got = String::from_utf8(out).unwrap().trim_end_matches('\n').to_owned();
    let audit = h.dom.audit().mismatches() as u32;
    let reported = h.reported;
    h.close(&mut e);
    assert_eq!(audit, 0, "{}: the counts audit", case.file);
    (got, reported)
}

#[test]
fn the_six_scripted_cases_build_the_expected_trees() {
    let cases = cases();
    assert_eq!(cases.len(), 6, "the four files hold six cases");
    let mut failed = Vec::new();
    for c in &cases {
        let (got, reported) = run(c);
        if got != c.document || reported != 0 {
            failed.push(format!("{} {:?}\n--- got ({reported} reported)\n{got}\n--- want\n{}", c.file, c.data, c.document));
        }
    }
    assert!(failed.is_empty(), "{} of 6 failed:\n{}", failed.len(), failed.join("\n\n"));
}
