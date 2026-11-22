//! A stylesheet's bytes to its text (CSS Syntax Level 3, 3.2): a byte
//! order mark decides; else the protocol's charset, an `@charset` rule
//! spelled exactly as the standard has it, the referring document's
//! encoding, and UTF-8, in that order.

use madcawl_html::{Decoder, Encoding};

const CHARSET: &[u8] = b"@charset \"";

/// The label of an `@charset "...";` at the very start, in the first
/// 1024 bytes.
fn charset_label(bytes: &[u8]) -> Option<&[u8]> {
    let rest = bytes.get(..1024.min(bytes.len()))?.strip_prefix(CHARSET)?;
    let end = rest.windows(2).position(|w| w == b"\";")?;
    Some(&rest[..end])
}

fn fallback(bytes: &[u8], protocol: Option<&[u8]>, environment: Option<Encoding>) -> Encoding {
    if let Some(e) = protocol.and_then(Encoding::for_label) {
        return e;
    }
    if let Some(e) = charset_label(bytes).and_then(Encoding::for_label) {
        return match e {
            Encoding::Utf16be | Encoding::Utf16le => Encoding::Utf8,
            e => e,
        };
    }
    environment.unwrap_or(Encoding::Utf8)
}

/// The text of a stylesheet and the encoding it was decoded from.
/// `protocol` is the label the response gave, `environment` the
/// encoding of the document that refers to the stylesheet.
pub fn decode_stylesheet(
    bytes: &[u8],
    protocol: Option<&[u8]>,
    environment: Option<Encoding>,
) -> (String, Encoding) {
    let (encoding, body) = match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => (Encoding::Utf8, rest),
        [0xFE, 0xFF, rest @ ..] => (Encoding::Utf16be, rest),
        [0xFF, 0xFE, rest @ ..] => (Encoding::Utf16le, rest),
        _ => (fallback(bytes, protocol, environment), bytes),
    };
    let mut out = Vec::with_capacity(body.len());
    Decoder::new(encoding).decode(body, &mut out, true);
    let text = String::from_utf8(out).unwrap_or_else(|e| {
        // the decoders write only UTF-8
        unreachable!("decoder output: {e}")
    });
    (text, encoding)
}
