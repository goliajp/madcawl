//! The names an `@layer` rule's prelude gives.

use crate::Span;
use crate::tokenizer::{Kind, Tokenizer};

/// The names of `@layer a, b.c`, each dotted name as one string;
/// `None` when the prelude is not a list of them.
pub(crate) fn layer_names(src: &str, span: Span) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    // a name part comes next: first, after a dot or after a comma
    let mut part = true;
    let mut spaced = false;
    let mut t = Tokenizer::in_span(src, span);
    loop {
        let tok = t.next_token();
        match tok.kind {
            Kind::Whitespace => {
                spaced = true;
                continue;
            }
            Kind::Ident if part && (cur.is_empty() || !spaced) => {
                tok.write_value(src, &mut cur);
                part = false;
            }
            Kind::Delim(b'.') if !part && !spaced => {
                cur.push('.');
                part = true;
            }
            Kind::Comma if !part => {
                out.push(std::mem::take(&mut cur));
                part = true;
            }
            Kind::Eof if !part || out.is_empty() && cur.is_empty() => break,
            _ => return None,
        }
        spaced = false;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Some(out)
}
