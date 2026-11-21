//! The two targets: each parses one input fed one way and returns what
//! came out, flattened to bytes, and the work it took.

use madcawl_html::{Flow, Options, ParseError, Sink, Token, Tokenizer, TreeBuilder, steps};

use crate::dom::{Dom, print};
use crate::wpt::{Feed, feed_all};

/// Every token and error in order, text runs merged, so the stream does
/// not depend on where the input was cut.
#[derive(Default)]
struct Flat {
    out: Vec<u8>,
    in_chars: bool,
}

impl Flat {
    fn put(&mut self, kind: u8, parts: &[&[u8]]) {
        self.in_chars = false;
        self.out.push(kind);
        for p in parts {
            self.out.extend_from_slice(&(p.len() as u32).to_le_bytes());
            self.out.extend_from_slice(p);
        }
    }
}

impl Sink for Flat {
    fn token(&mut self, t: Token<'_>) -> Flow {
        match t {
            Token::Chars(s) => {
                if !self.in_chars {
                    self.out.push(b'c');
                    self.in_chars = true;
                }
                self.out.extend_from_slice(s);
            }
            Token::StartTag(tag) | Token::EndTag(tag) => {
                let kind = if matches!(t, Token::StartTag(_)) {
                    b's'
                } else {
                    b'e'
                };
                self.put(kind, &[&tag.name, &[tag.self_closing as u8]]);
                for a in &tag.attrs {
                    self.put(b'a', &[&a.name, &a.value]);
                }
            }
            Token::Comment(s) => self.put(b'#', &[s]),
            Token::ProcessingInstruction { target, data } => self.put(b'?', &[target, data]),
            Token::Doctype(d) => {
                let part = |p: &Option<Vec<u8>>| {
                    p.clone().map(|mut v| {
                        v.insert(0, b'+');
                        v
                    })
                };
                let (n, p, s) = (part(&d.name), part(&d.public_id), part(&d.system_id));
                let none = Vec::new();
                self.put(
                    b'!',
                    &[
                        n.as_ref().unwrap_or(&none),
                        p.as_ref().unwrap_or(&none),
                        s.as_ref().unwrap_or(&none),
                        &[d.force_quirks as u8],
                    ],
                );
            }
            Token::Eof => self.put(b'$', &[]),
        }
        Flow::Continue
    }

    fn error(&mut self, e: ParseError) {
        self.put(b'x', &[e.code().as_bytes()]);
    }
}

pub fn tokenizer(input: &[u8], feed: Feed) -> (Vec<u8>, u64) {
    steps::take();
    let mut tok = Tokenizer::new();
    let mut sink = Flat::default();
    feed_all(&mut tok, &mut sink, input, feed);
    (sink.out, steps::take())
}

pub fn tree(input: &[u8], feed: Feed) -> (Vec<u8>, u64) {
    steps::take();
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(Dom::new(), Options::default());
    feed_all(&mut tok, &mut b, input, feed);
    let n = steps::take();
    let mut out = Vec::new();
    print(&b.into_sink(), 0, 0, &mut out);
    (out, n)
}
