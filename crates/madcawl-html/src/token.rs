//! What the tokenizer hands the tree builder.
//!
//! Text is WTF-8: UTF-8 that may also carry lone surrogates, which a
//! script can put into a document and a decoder never does.

/// A start or end tag. Names are ASCII-lowercased as the standard
/// says; anything else in them is kept as written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tag {
    pub name: Vec<u8>,
    pub attrs: Vec<Attribute>,
    pub self_closing: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attribute {
    pub name: Vec<u8>,
    pub value: Vec<u8>,
}

/// A DOCTYPE. `None` is a part that was missing, which is not the same
/// as one that was given empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Doctype {
    pub name: Option<Vec<u8>>,
    pub public_id: Option<Vec<u8>>,
    pub system_id: Option<Vec<u8>>,
    pub force_quirks: bool,
}

/// One token, borrowed from the tokenizer for as long as the sink
/// looks at it. Character data comes in runs; two runs in a row are
/// one stretch of text.
#[derive(Debug, PartialEq, Eq)]
pub enum Token<'a> {
    Chars(&'a [u8]),
    StartTag(&'a Tag),
    EndTag(&'a Tag),
    Comment(&'a [u8]),
    /// `<?target data?>`. The standard parses these since 2026.
    ProcessingInstruction {
        target: &'a [u8],
        data: &'a [u8],
    },
    Doctype(&'a Doctype),
    Eof,
}
