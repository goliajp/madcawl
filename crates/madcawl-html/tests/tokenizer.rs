//! The html5lib tokenizer tests, every one of them, run twice: with
//! the whole input at once, and fed one byte at a time.

mod support;

use madcawl_html::{Flow, ParseError, Sink, State, Step, Token, Tokenizer};
use support::json::{Json, parse, unescape};

#[derive(Debug, PartialEq)]
enum Out {
    Chars(Vec<u8>),
    Start(Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>, bool),
    End(Vec<u8>),
    Comment(Vec<u8>),
    Pi(Vec<u8>, Vec<u8>),
    Doctype(Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>, bool),
}

#[derive(Default)]
struct Collect {
    out: Vec<Out>,
    errors: Vec<&'static str>,
    eof: bool,
}

impl Sink for Collect {
    fn token(&mut self, t: Token<'_>) -> Flow {
        match t {
            Token::Chars(s) => {
                if let Some(Out::Chars(prev)) = self.out.last_mut() {
                    prev.extend_from_slice(s);
                } else {
                    self.out.push(Out::Chars(s.to_vec()));
                }
            }
            Token::StartTag(tag) => self.out.push(Out::Start(
                tag.name.clone(),
                tag.attrs
                    .iter()
                    .map(|a| (a.name.clone(), a.value.clone()))
                    .collect(),
                tag.self_closing,
            )),
            Token::EndTag(tag) => self.out.push(Out::End(tag.name.clone())),
            Token::Comment(s) => self.out.push(Out::Comment(s.to_vec())),
            Token::ProcessingInstruction { target, data } => {
                self.out.push(Out::Pi(target.to_vec(), data.to_vec()))
            }
            Token::Doctype(d) => self.out.push(Out::Doctype(
                d.name.clone(),
                d.public_id.clone(),
                d.system_id.clone(),
                !d.force_quirks,
            )),
            Token::Eof => self.eof = true,
        }
        Flow::Continue
    }
    fn error(&mut self, e: ParseError) {
        self.errors.push(e.code());
    }
}

fn state_named(name: &[u8]) -> State {
    match name {
        b"Data state" => State::Data,
        b"PLAINTEXT state" => State::Plaintext,
        b"RCDATA state" => State::Rcdata,
        b"RAWTEXT state" => State::Rawtext,
        b"Script data state" => State::ScriptData,
        b"CDATA section state" => State::CdataSection,
        _ => panic!("unknown state {}", String::from_utf8_lossy(name)),
    }
}

fn tokenize(input: &[u8], state: State, last_start: Option<&[u8]>, bytewise: bool) -> Collect {
    let mut t = Tokenizer::new();
    t.set_state(state);
    if let Some(name) = last_start {
        t.set_last_start_tag(name);
    }
    let mut sink = Collect::default();
    if bytewise {
        for b in input {
            t.feed(std::slice::from_ref(b));
            assert_eq!(t.run(&mut sink), Step::NeedInput);
        }
    } else {
        t.feed(input);
    }
    t.end();
    assert_eq!(t.run(&mut sink), Step::Done);
    assert!(sink.eof);
    sink
}

fn expected(test: &Json, double: bool) -> Vec<Out> {
    let s = |j: &Json| -> Vec<u8> {
        let v = j.str().expect("a string").to_vec();
        if double { unescape(&v) } else { v }
    };
    let opt = |j: &Json| j.str().map(|_| s(j));
    let mut out: Vec<Out> = Vec::new();
    for tok in test.get("output").expect("output").arr() {
        let a = tok.arr();
        let kind = a[0].str().expect("kind");
        let o = match kind {
            b"Character" => {
                if let Some(Out::Chars(prev)) = out.last_mut() {
                    prev.extend_from_slice(&s(&a[1]));
                    continue;
                }
                Out::Chars(s(&a[1]))
            }
            b"StartTag" => {
                let attrs = match &a[2] {
                    Json::Obj(kv) => kv.iter().map(|(k, v)| (k.clone(), s(v))).collect(),
                    _ => Vec::new(),
                };
                Out::Start(s(&a[1]), attrs, a.get(3) == Some(&Json::Bool(true)))
            }
            b"EndTag" => Out::End(s(&a[1])),
            b"Comment" => Out::Comment(s(&a[1])),
            b"ProcessingInstruction" => Out::Pi(s(&a[1]), s(&a[2])),
            b"DOCTYPE" => {
                Out::Doctype(opt(&a[1]), opt(&a[2]), opt(&a[3]), a[4] == Json::Bool(true))
            }
            _ => panic!("unknown token kind"),
        };
        out.push(o);
    }
    out
}

#[test]
fn html5lib_tokenizer_tests() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/html5lib-tokenizer");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let (mut ran, mut failures) = (0, Vec::new());
    for path in files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "test"))
    {
        let doc = parse(&std::fs::read_to_string(path).unwrap());
        let name = path.file_name().unwrap().to_string_lossy();
        // xmlViolation.test is for a parser in XML-compatible mode
        let Some(tests) = doc.get("tests") else {
            eprintln!("file {name} runs 0 failed 0 skipped as xml");
            continue;
        };
        let (ran_before, failed_before) = (ran, failures.len());
        for test in tests.arr() {
            let double = test.get("doubleEscaped") == Some(&Json::Bool(true));
            let mut input = test
                .get("input")
                .and_then(Json::str)
                .expect("input")
                .to_vec();
            if double {
                input = unescape(&input);
            }
            let want = expected(test, double);
            let want_errors: Vec<Vec<u8>> = test
                .get("errors")
                .map(|e| {
                    e.arr()
                        .iter()
                        .map(|x| x.get("code").and_then(Json::str).unwrap().to_vec())
                        .collect()
                })
                .unwrap_or_default();
            let states: Vec<State> = match test.get("initialStates") {
                Some(s) => s
                    .arr()
                    .iter()
                    .map(|n| state_named(n.str().unwrap()))
                    .collect(),
                None => vec![State::Data],
            };
            let last = test.get("lastStartTag").and_then(Json::str);
            for &state in &states {
                for bytewise in [false, true] {
                    ran += 1;
                    let got = tokenize(&input, state, last, bytewise);
                    let got_errors: Vec<Vec<u8>> =
                        got.errors.iter().map(|c| c.as_bytes().to_vec()).collect();
                    if got.out != want || got_errors != want_errors {
                        failures.push(format!(
                            "{} / {} / {state:?}{}\n  input  {:?}\n  want   {:?}\n  got    {:?}\n  errors want {:?}\n  errors got  {:?}",
                            path.file_name().unwrap().to_string_lossy(),
                            String::from_utf8_lossy(test.get("description").and_then(Json::str).unwrap_or(b"")),
                            if bytewise { " / byte at a time" } else { "" },
                            String::from_utf8_lossy(&input),
                            want,
                            got.out,
                            want_errors.iter().map(|e| String::from_utf8_lossy(e).into_owned()).collect::<Vec<_>>(),
                            got.errors,
                        ));
                    }
                }
            }
        }
        eprintln!(
            "file {name} runs {} failed {}",
            ran - ran_before,
            failures.len() - failed_before
        );
    }
    for f in failures.iter().take(15) {
        eprintln!("{f}\n");
    }
    assert!(
        failures.is_empty(),
        "{} of {ran} runs failed",
        failures.len()
    );
    eprintln!("{ran} runs passed");
}
