//! WHATWG HTML parsing: the tokenizer and the tree builder.

/// Count `n` units of work for the `steps` feature; nothing without it.
macro_rules! step {
    ($n:expr) => {
        #[cfg(feature = "steps")]
        $crate::steps::add($n as u64);
    };
}

#[cfg(feature = "steps")]
pub mod steps;

mod document;
pub mod encoding;
mod entities;
mod error;
mod input;
mod token;
mod tokenizer;
mod tree;

pub use document::{DocumentParser, Source};
pub use encoding::{Confidence, Decoder, Encoding};
pub use error::ParseError;
pub use madcawl_str::{Atom, HeldAtoms, atom};
pub use token::{Attribute, Doctype, Tag, Token};
pub use tokenizer::{Flow, Sink, State, Step, Tokenizer};
pub use tree::{
    AttrNamespace, AttrRef, Attrs, FragmentContext, Namespace, Options, QuirksMode, TreeBuilder,
    TreeSink,
};
