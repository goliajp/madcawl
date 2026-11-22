//! Component values as a tree: what CSSOM-facing code and the tests
//! want when they need the structure of a value, not only its span.
//! Built with an explicit stack, so nesting depth costs heap, not
//! native stack.

use crate::Span;
use crate::parser::{SyntaxError, closer};
use crate::tokenizer::{Kind, Token, Tokenizer};

#[derive(Clone, Debug, PartialEq)]
pub enum ComponentValue {
    Token(Token),
    /// A `{}`, `[]` or `()` block or a function: its opening token and
    /// its contents. A block the input ended inside has no closing token
    Block {
        open: Token,
        contents: Vec<ComponentValue>,
    },
}

/// Dropping takes the nested blocks apart with a stack of its own, so a
/// value nested as deep as its input allows does not recurse.
impl Drop for ComponentValue {
    fn drop(&mut self) {
        let ComponentValue::Block { contents, .. } = self else {
            return;
        };
        let mut stack = std::mem::take(contents);
        while let Some(mut v) = stack.pop() {
            if let ComponentValue::Block { contents, .. } = &mut v {
                stack.append(contents);
            }
        }
    }
}

/// Consume component values; after the first one when `one`.
fn consume(tok: &mut Tokenizer<'_>, one: bool) -> Vec<ComponentValue> {
    let mut stack: Vec<(Token, Kind, Vec<ComponentValue>)> = Vec::new();
    let mut cur = Vec::new();
    loop {
        if one && stack.is_empty() && !cur.is_empty() {
            return cur;
        }
        let t = tok.next_token();
        if t.kind == Kind::Eof {
            while let Some((open, _, mut parent)) = stack.pop() {
                parent.push(ComponentValue::Block {
                    open,
                    contents: cur,
                });
                cur = parent;
            }
            return cur;
        }
        if let Some((open, _, mut parent)) = stack.pop_if(|top| top.1 == t.kind) {
            parent.push(ComponentValue::Block {
                open,
                contents: std::mem::take(&mut cur),
            });
            cur = parent;
            continue;
        }
        if let Some(c) = closer(t.kind) {
            stack.push((t, c, std::mem::take(&mut cur)));
            continue;
        }
        cur.push(ComponentValue::Token(t));
    }
}

pub fn parse_component_value_list(src: &str, span: Span) -> Vec<ComponentValue> {
    consume(&mut Tokenizer::in_span(src, span), false)
}

pub fn parse_component_value(src: &str) -> Result<ComponentValue, SyntaxError> {
    let mut tok = Tokenizer::new(src);
    let skip_ws = |tok: &mut Tokenizer<'_>| loop {
        let at = tok.position();
        if tok.next_token().kind != Kind::Whitespace {
            tok.reset(at);
            return;
        }
    };
    skip_ws(&mut tok);
    let at = tok.position();
    if tok.next_token().kind == Kind::Eof {
        return Err(SyntaxError::Empty);
    }
    tok.reset(at);
    let v = consume(&mut tok, true).pop().expect("one value");
    skip_ws(&mut tok);
    match tok.next_token().kind {
        Kind::Eof => Ok(v),
        _ => Err(SyntaxError::ExtraInput),
    }
}
