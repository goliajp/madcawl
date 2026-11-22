//! Decoders from an encoding to UTF-8, a chunk at a time. What they
//! write is always valid UTF-8, so it is WTF-8 the tokenizer can read
//! as it is.

use super::Encoding;

const REPLACEMENT: &[u8] = "\u{FFFD}".as_bytes();

/// A decoder for one encoding, keeping what a chunk ended in the
/// middle of for the next one. Errors decode to U+FFFD.
#[derive(Clone, Debug)]
pub struct Decoder {
    kind: Kind,
}

#[derive(Clone, Debug)]
enum Kind {
    Utf8(Utf8),
    Utf16 {
        big: bool,
        lead_byte: Option<u8>,
        lead_surrogate: Option<u16>,
    },
    XUserDefined,
    /// The replacement encoding: one U+FFFD for any input at all
    Replacement {
        written: bool,
    },
    /// Every other encoding
    Legacy,
}

impl Decoder {
    pub fn new(encoding: Encoding) -> Self {
        let kind = match encoding {
            Encoding::Utf8 => Kind::Utf8(Utf8::default()),
            Encoding::Utf16be | Encoding::Utf16le => Kind::Utf16 {
                big: encoding == Encoding::Utf16be,
                lead_byte: None,
                lead_surrogate: None,
            },
            Encoding::XUserDefined => Kind::XUserDefined,
            Encoding::Replacement => Kind::Replacement { written: false },
            _ => Kind::Legacy,
        };
        Decoder { kind }
    }

    /// Decode `input`, appending to `out`. `last` when no more input
    /// follows: what is left unfinished then is an error.
    pub fn decode(&mut self, input: &[u8], out: &mut Vec<u8>, last: bool) {
        match &mut self.kind {
            Kind::Utf8(d) => d.decode(input, out, last),
            Kind::Utf16 {
                big,
                lead_byte,
                lead_surrogate,
            } => utf16(*big, lead_byte, lead_surrogate, input, out, last),
            Kind::XUserDefined => {
                for &b in input {
                    let c = if b < 0x80 {
                        u32::from(b)
                    } else {
                        0xF780 + u32::from(b) - 0x80
                    };
                    push(out, c);
                }
            }
            Kind::Replacement { written } => {
                if !input.is_empty() && !*written {
                    *written = true;
                    out.extend_from_slice(REPLACEMENT);
                }
            }
            Kind::Legacy => legacy(input, out),
        }
    }
}

/// The legacy single-byte and multi-byte encodings need their index
/// tables, which this crate does not carry yet: ASCII decodes as
/// itself, every other byte as U+FFFD.
fn legacy(input: &[u8], out: &mut Vec<u8>) {
    for &b in input {
        if b < 0x80 {
            out.push(b);
        } else {
            out.extend_from_slice(REPLACEMENT);
        }
    }
}

/// The standard's UTF-8 decoder. A sequence that goes wrong is one
/// error as far as it was valid, and the byte that broke it is read
/// again on its own, so `E2 82 41` is U+FFFD then `A`.
#[derive(Clone, Debug)]
struct Utf8 {
    code_point: u32,
    seen: u8,
    needed: u8,
    lower: u8,
    upper: u8,
}

impl Default for Utf8 {
    fn default() -> Self {
        Utf8 {
            code_point: 0,
            seen: 0,
            needed: 0,
            lower: 0x80,
            upper: 0xBF,
        }
    }
}

impl Utf8 {
    fn decode(&mut self, mut input: &[u8], out: &mut Vec<u8>, last: bool) {
        // finish a sequence the last chunk cut off, a byte at a time
        while self.needed != 0 {
            let Some((&b, rest)) = input.split_first() else {
                if last {
                    *self = Utf8::default();
                    out.extend_from_slice(REPLACEMENT);
                }
                return;
            };
            if self.byte(b, out) {
                input = rest;
            }
        }
        // whole valid runs through the standard library's validator,
        // whose error lengths are the same maximal subparts
        loop {
            match std::str::from_utf8(input) {
                Ok(s) => {
                    out.extend_from_slice(s.as_bytes());
                    return;
                }
                Err(e) => {
                    let (valid, rest) = input.split_at(e.valid_up_to());
                    out.extend_from_slice(valid);
                    match e.error_len() {
                        Some(n) => {
                            out.extend_from_slice(REPLACEMENT);
                            input = &rest[n..];
                        }
                        None => {
                            // a valid start of a sequence the chunk cuts off
                            for &b in rest {
                                let used = self.byte(b, out);
                                debug_assert!(used);
                            }
                            if last {
                                *self = Utf8::default();
                                out.extend_from_slice(REPLACEMENT);
                            }
                            return;
                        }
                    }
                }
            }
        }
    }

    /// One byte through the standard's handler. `false` when the byte
    /// ended a broken sequence and must be read again.
    fn byte(&mut self, b: u8, out: &mut Vec<u8>) -> bool {
        if self.needed == 0 {
            match b {
                0x00..=0x7F => out.push(b),
                0xC2..=0xDF => {
                    self.needed = 1;
                    self.code_point = u32::from(b & 0x1F);
                }
                0xE0..=0xEF => {
                    if b == 0xE0 {
                        self.lower = 0xA0;
                    }
                    if b == 0xED {
                        self.upper = 0x9F;
                    }
                    self.needed = 2;
                    self.code_point = u32::from(b & 0x0F);
                }
                0xF0..=0xF4 => {
                    if b == 0xF0 {
                        self.lower = 0x90;
                    }
                    if b == 0xF4 {
                        self.upper = 0x8F;
                    }
                    self.needed = 3;
                    self.code_point = u32::from(b & 0x07);
                }
                _ => out.extend_from_slice(REPLACEMENT),
            }
            return true;
        }
        if !(self.lower..=self.upper).contains(&b) {
            *self = Utf8::default();
            out.extend_from_slice(REPLACEMENT);
            return false;
        }
        self.lower = 0x80;
        self.upper = 0xBF;
        self.code_point = (self.code_point << 6) | u32::from(b & 0x3F);
        self.seen += 1;
        if self.seen == self.needed {
            push(out, self.code_point);
            *self = Utf8::default();
        }
        true
    }
}

/// The standard's shared UTF-16 decoder.
fn utf16(
    big: bool,
    lead_byte: &mut Option<u8>,
    lead_surrogate: &mut Option<u16>,
    input: &[u8],
    out: &mut Vec<u8>,
    last: bool,
) {
    for &b in input {
        let Some(lead) = lead_byte.take() else {
            *lead_byte = Some(b);
            continue;
        };
        let unit = if big {
            u16::from_be_bytes([lead, b])
        } else {
            u16::from_le_bytes([lead, b])
        };
        if let Some(high) = lead_surrogate.take() {
            if (0xDC00..=0xDFFF).contains(&unit) {
                let c = 0x1_0000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(unit) - 0xDC00);
                push(out, c);
                continue;
            }
            // the unit is read again after the error
            out.extend_from_slice(REPLACEMENT);
        }
        match unit {
            0xD800..=0xDBFF => *lead_surrogate = Some(unit),
            0xDC00..=0xDFFF => out.extend_from_slice(REPLACEMENT),
            _ => push(out, u32::from(unit)),
        }
    }
    if last && (lead_byte.is_some() || lead_surrogate.is_some()) {
        *lead_byte = None;
        *lead_surrogate = None;
        out.extend_from_slice(REPLACEMENT);
    }
}

/// Append a scalar value as UTF-8.
fn push(out: &mut Vec<u8>, c: u32) {
    let c = char::from_u32(c).expect("the decoders make only scalar values");
    out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
}
