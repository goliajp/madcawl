//! The rules for parsing tokens in foreign content.

use super::tag::Tag;
use super::{Class, Namespace, Step, Tok, TreeBuilder, TreeSink};

const REPLACEMENT: &[u8] = "\u{FFFD}".as_bytes();

fn breaks_out(tag: Tag) -> bool {
    use Tag::*;
    matches!(
        tag,
        B | Big
            | Blockquote
            | Body
            | Br
            | Center
            | Code
            | Dd
            | Div
            | Dl
            | Dt
            | Em
            | Embed
            | H1
            | H2
            | H3
            | H4
            | H5
            | H6
            | Head
            | Hr
            | I
            | Img
            | Li
            | Listing
            | Menu
            | Meta
            | Nobr
            | Ol
            | P
            | Pre
            | Ruby
            | S
            | Small
            | Span
            | Strong
            | Strike
            | Sub
            | Sup
            | Table
            | Tt
            | U
            | Ul
            | Var
    )
}

impl<S: TreeSink> TreeBuilder<S> {
    pub(crate) fn foreign(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Text(Class::Nul, s) => {
                for _ in s {
                    self.insert_chars(REPLACEMENT);
                }
            }
            Tok::Text(class, s) => {
                self.insert_chars(s);
                if class == Class::Other {
                    self.frameset_ok = false;
                }
            }
            Tok::Comment(d) => self.insert_comment(d, None),
            Tok::Pi(t, d) => self.insert_pi(t, d, None),
            Tok::Doctype(_) | Tok::Eof => {}
            Tok::Start(s)
                if breaks_out(s.name.tag)
                    || (s.name.tag == Tag::Font
                        && s.attrs
                            .iter()
                            .any(|a| matches!(&a.name[..], b"color" | b"face" | b"size"))) =>
            {
                return self.break_out();
            }
            Tok::End(n) if matches!(n.tag, Tag::Br | Tag::P) => return self.break_out(),
            Tok::Start(s) => {
                let ns = self
                    .adjusted_current()
                    .expect("an adjusted current node")
                    .ns;
                self.insert_element(&s, ns);
                if s.self_closing {
                    self.pop();
                }
            }
            Tok::End(n) => {
                let cur = self.current();
                if n.tag == Tag::Script && cur.is(Namespace::Svg, Tag::Script) {
                    self.pop();
                    return Step::Done;
                }
                let mut i = self.open.len() - 1;
                loop {
                    step!(1);
                    if i == 0 {
                        return Step::Done;
                    }
                    if self.open[i].name == n {
                        while self.open.len() > i {
                            self.pop();
                        }
                        return Step::Done;
                    }
                    i -= 1;
                    if self.open[i].ns == Namespace::Html {
                        return self.by_mode(self.mode, tok);
                    }
                }
            }
        }
        Step::Done
    }

    fn break_out(&mut self) -> Step {
        while let Some(e) = self.open.last() {
            if e.mathml_text_ip() || e.html_ip || e.ns == Namespace::Html {
                break;
            }
            self.pop();
        }
        Step::AgainHtml
    }
}
