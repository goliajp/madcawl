//! Encodings: which one a document is in (HTML 13.2.3) and turning its
//! bytes into the WTF-8 the tokenizer reads.

mod decode;
mod prescan;

pub use decode::Decoder;
pub use prescan::{extract_from_content, prescan};

pub use torajs_encoding::{DecodeError, Encoding};

pub(crate) fn is_utf16(e: Encoding) -> bool {
    matches!(e, Encoding::Utf16be | Encoding::Utf16le)
}

/// How sure the parser is of the encoding (13.2.3.1): a tentative one
/// may still change when the parser meets a `meta` that names another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    Tentative,
    Certain,
}

/// The encoding a byte order mark at the start of `bytes` names, and
/// how long the mark is. `None` when too few bytes have arrived to
/// tell and more may come.
pub(crate) fn bom(bytes: &[u8], done: bool) -> Option<Option<(Encoding, usize)>> {
    let found = match bytes {
        [0xEF, 0xBB, 0xBF, ..] => Some((Encoding::Utf8, 3)),
        [0xFE, 0xFF, ..] => Some((Encoding::Utf16be, 2)),
        [0xFF, 0xFE, ..] => Some((Encoding::Utf16le, 2)),
        [0xEF] | [0xEF, 0xBB] | [0xFE] | [0xFF] | [] if !done => return None,
        _ => None,
    };
    Some(found)
}
