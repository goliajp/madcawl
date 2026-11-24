//! A declaration's value as the property parsers read it: its top-level
//! component values with whitespace dropped, a function or block kept
//! as one item with the span of its contents.

use crate::Span;
use crate::parser::Parser;
use crate::tokenizer::{Kind, Token};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Item {
    pub tok: Token,
    /// What is inside a function or block; empty for other tokens
    pub inner: Span,
}

impl Item {
    pub fn kind(&self) -> Kind {
        self.tok.kind
    }

    /// A lowercase identifier, if the item is one.
    pub fn ident(&self, src: &str) -> Option<String> {
        (self.tok.kind == Kind::Ident).then(|| {
            let mut s = String::new();
            self.tok.write_value(src, &mut s);
            s.make_ascii_lowercase();
            s
        })
    }

    pub fn is_function(&self, src: &str, name: &str) -> bool {
        self.tok.kind == Kind::Function && self.tok.value_is(src, name)
    }

    pub fn is_comma(&self) -> bool {
        self.tok.kind == Kind::Comma
    }

    pub fn is_delim(&self, c: u8) -> bool {
        self.tok.kind == Kind::Delim(c)
    }

    /// From the start of the item to its end, a function's closing
    /// parenthesis included.
    pub fn whole_in(&self, src: &str) -> Span {
        if matches!(
            self.tok.kind,
            Kind::Function | Kind::OpenParen | Kind::OpenSquare | Kind::OpenCurly
        ) {
            let e = self.inner.end as usize;
            // a block the input ended inside has no closing token
            let closed = matches!(src.as_bytes().get(e), Some(b')' | b']' | b'}'));
            let end = e + usize::from(closed);
            Span::new(self.tok.span.start as usize, end)
        } else {
            self.tok.span
        }
    }
}

/// The items of `src[span]`, at most `cap`: a value longer than that is
/// none the property parsers take.
pub(crate) fn items(src: &str, span: Span, cap: usize) -> Option<Vec<Item>> {
    let mut p = Parser::new(src, span);
    let mut out = Vec::new();
    loop {
        let tok = p.next_token();
        match tok.kind {
            Kind::Eof => return Some(out),
            Kind::Whitespace => continue,
            Kind::Function | Kind::OpenParen | Kind::OpenSquare | Kind::OpenCurly => {
                let end = p.skip_block(tok.kind);
                let inner = Span::new(tok.span.end as usize, end);
                out.push(Item { tok, inner });
            }
            _ => out.push(Item {
                tok,
                inner: Span::new(tok.span.end as usize, tok.span.end as usize),
            }),
        }
        if out.len() > cap {
            return None;
        }
    }
}
