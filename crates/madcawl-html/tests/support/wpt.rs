//! The html5lib tree construction format, and the three ways the
//! tests feed an input to the tokenizer.

use madcawl_dom::{Audit, Dom};
use madcawl_html::{
    Attrs, FragmentContext, HeldAtoms, Namespace, Options, QuirksMode, Sink, Step, Tokenizer,
    TreeBuilder, TreeSink, atom,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feed {
    Whole,
    Bytewise,
    /// Chunks of 1 to `max` bytes, sizes from a fixed seed, so a chunk
    /// often ends inside a multi-byte character or a character reference
    Chunks {
        seed: u64,
        max: usize,
    },
}

/// xorshift64*: deterministic, no dependency
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

pub fn feed_all<S: Sink>(tok: &mut Tokenizer, sink: &mut S, data: &[u8], feed: Feed) {
    match feed {
        Feed::Whole => tok.feed(data),
        Feed::Bytewise => {
            for b in data {
                tok.feed(std::slice::from_ref(b));
                assert_eq!(tok.run(sink), Step::NeedInput);
            }
        }
        Feed::Chunks { seed, max } => {
            let mut rng = Rng(seed | 1);
            let mut rest = data;
            while !rest.is_empty() {
                let n = (1 + rng.next() as usize % max).min(rest.len());
                tok.feed(&rest[..n]);
                assert_eq!(tok.run(sink), Step::NeedInput);
                rest = &rest[n..];
            }
        }
    }
    tok.end();
    assert_eq!(tok.run(sink), Step::Done);
}

#[derive(Default)]
pub struct Test {
    pub data: Vec<u8>,
    pub fragment: Option<Vec<u8>>,
    pub script: Option<bool>,
    pub document: Vec<u8>,
    pub line: usize,
}

pub fn parse_tests(text: &[u8]) -> Vec<Test> {
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

/// The tree the parser builds for `test`, in the test format, and the
/// count audit of the DOM it built.
pub fn run(test: &Test, scripting: bool, feed: Feed) -> (Vec<u8>, Audit) {
    let opts = Options {
        scripting,
        ..Options::default()
    };
    let mut dom = Dom::new();
    let mut tok = Tokenizer::new();
    let names = HeldAtoms::new();
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
                .then(|| dom.create_element(Namespace::Html, atom!("form"), Attrs::empty()));
            let b = TreeBuilder::new_fragment(
                dom,
                opts,
                FragmentContext {
                    ns,
                    local: names.hold(local),
                    attrs: &[],
                    form,
                    quirks: QuirksMode::NoQuirks,
                },
            );
            tok.set_state(b.tokenizer_state());
            b
        }
    };
    feed_all(&mut tok, &mut builder, &test.data, feed);
    let dom = builder.into_sink();
    // a fragment's nodes are the children of the root element the
    // builder put into the document
    let root = match test.fragment {
        None => dom.document(),
        Some(_) => dom
            .first_child(dom.document())
            .expect("the fragment's root"),
    };
    let mut out = Vec::new();
    dom.write_dat(root, &mut out);
    out.pop();
    (out, dom.audit())
}
