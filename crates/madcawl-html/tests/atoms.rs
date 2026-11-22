//! The tokenizer gives the static atom of every tag and attribute name
//! that has one, however the input is cut.

use madcawl_html::{Atom, Flow, Sink, Step, Token, Tokenizer, atom};

#[derive(Default)]
struct Names(Vec<(Vec<u8>, Option<Atom>)>);

impl Sink for Names {
    fn token(&mut self, token: Token<'_>) -> Flow {
        if let Token::StartTag(t) | Token::EndTag(t) = token {
            self.0.push((t.name.clone(), t.atom));
            for a in &t.attrs {
                self.0.push((a.name.clone(), a.atom));
            }
        }
        Flow::Continue
    }
}

fn names(input: &[u8], chunk: usize) -> Vec<(Vec<u8>, Option<Atom>)> {
    let mut tok = Tokenizer::new();
    let mut sink = Names::default();
    for piece in input.chunks(chunk) {
        tok.feed(piece);
        assert_eq!(tok.run(&mut sink), Step::NeedInput);
    }
    tok.end();
    assert_eq!(tok.run(&mut sink), Step::Done);
    sink.0
}

#[test]
fn names_get_their_static_atoms_in_every_feeding() {
    let input = b"<DIV Class=x DATA-foo=1 =eq><svg viewBox=0></Svg><x-y\0><title>t</TITLE>";
    let want = vec![
        (b"div".to_vec(), Some(atom!("div"))),
        (b"class".to_vec(), Some(atom!("class"))),
        (b"data-foo".to_vec(), None),
        (b"=eq".to_vec(), None),
        (b"svg".to_vec(), Some(atom!("svg"))),
        (b"viewbox".to_vec(), Some(atom!("viewbox"))),
        (b"svg".to_vec(), Some(atom!("svg"))),
        ("x-y\u{FFFD}".as_bytes().to_vec(), None),
        (b"title".to_vec(), Some(atom!("title"))),
        (b"title".to_vec(), Some(atom!("title"))),
    ];
    for chunk in [input.len(), 1, 3, 7] {
        assert_eq!(names(input, chunk), want, "chunks of {chunk}");
    }
}
