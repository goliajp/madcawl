//! WHATWG HTML parsing: the tokenizer and the tree builder.

mod entities;
mod error;
mod input;
mod token;
mod tokenizer;

pub use error::ParseError;
pub use token::{Attribute, Doctype, Tag, Token};
pub use tokenizer::{Flow, Sink, State, Step, Tokenizer};
