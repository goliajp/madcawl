//! CSS: the tokenizer and parser of CSS Syntax Level 3 and the selectors
//! of Selectors Level 4.
//!
//! Everything works on spans of the stylesheet's one copy of its source.
//! The tokenizer is pulled a token at a time and allocates nothing; the
//! parser finds rules and declarations as spans, skipping blocks, so a
//! block is parsed again from its span only when something wants it.
//! The names selectors use are interned in a table of their own, and
//! turned into atoms on the main thread in one pass.

/// Count `n` units of work for the `steps` feature; nothing without it.
macro_rules! step {
    ($n:expr) => {
        #[cfg(feature = "steps")]
        $crate::steps::add($n as u64);
    };
}

#[cfg(feature = "steps")]
pub mod steps;

mod anb;
mod bytes;
mod escape;
mod names;
mod parser;
pub mod selector;
mod tokenizer;
mod tree;

pub use anb::parse_anb;
pub use bytes::decode_stylesheet;
pub use escape::Unescape;
pub use madcawl_html::Encoding;
pub use names::{Name, Names};
pub use parser::{
    BlockItem, Declaration, ParsedRule, Parser, SyntaxError, parse_block_contents,
    parse_declaration, parse_declaration_list, parse_rule, parse_rule_list,
};
pub use tokenizer::{AT_EOF, ESCAPED, INTEGER, Kind, Token, Tokenizer};
pub use tree::{ComponentValue, parse_component_value, parse_component_value_list};

/// A byte range of a stylesheet's source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        Span {
            start: start as u32,
            end: end as u32,
        }
    }

    pub fn of(self, src: &str) -> &str {
        &src[self.start as usize..self.end as usize]
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}
