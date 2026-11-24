//! Decoders from an encoding to UTF-8, a chunk at a time. What they
//! write is always valid UTF-8, so it is WTF-8 the tokenizer can read
//! as it is.

use torajs_encoding::big5::Big5Decoder;
use torajs_encoding::euc_jp::EucJpDecoder;
use torajs_encoding::euc_kr::EucKrDecoder;
use torajs_encoding::gb18030::Gb18030Decoder;
use torajs_encoding::iso_2022_jp::Iso2022JpDecoder;
use torajs_encoding::shift_jis::ShiftJisDecoder;
use torajs_encoding::single_byte;
use torajs_encoding::utf16::{Endian, Utf16Decoder};
use torajs_encoding::{Encoding, Family};

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
    Utf16(Endian, Utf16Decoder),
    SingleByte(&'static single_byte::Index),
    XUserDefined,
    Gb18030(Gb18030Decoder),
    Big5(Big5Decoder),
    EucJp(EucJpDecoder),
    Iso2022Jp(Iso2022JpDecoder),
    ShiftJis(ShiftJisDecoder),
    EucKr(EucKrDecoder),
    /// The replacement encoding: one U+FFFD for any input at all
    Replacement {
        written: bool,
    },
}

impl Decoder {
    pub fn new(encoding: Encoding) -> Self {
        let kind = match encoding.family() {
            Family::Utf8 => Kind::Utf8(Utf8::default()),
            Family::Utf16(endian) => Kind::Utf16(endian, Utf16Decoder::default()),
            Family::SingleByte(index) => Kind::SingleByte(index),
            Family::XUserDefined => Kind::XUserDefined,
            Family::Gb18030 => Kind::Gb18030(Gb18030Decoder::default()),
            Family::Big5 => Kind::Big5(Big5Decoder::default()),
            Family::EucJp => Kind::EucJp(EucJpDecoder::default()),
            Family::Iso2022Jp => Kind::Iso2022Jp(Iso2022JpDecoder::default()),
            Family::ShiftJis => Kind::ShiftJis(ShiftJisDecoder::default()),
            Family::EucKr => Kind::EucKr(EucKrDecoder::default()),
            _ => Kind::Replacement { written: false },
        };
        Decoder { kind }
    }

    /// Decode `input`, appending to `out`. `last` when no more input
    /// follows: what is left unfinished then is an error.
    pub fn decode(&mut self, input: &[u8], out: &mut Vec<u8>, last: bool) {
        let r = match &mut self.kind {
            Kind::Utf8(d) => {
                d.decode(input, out, last);
                Ok(())
            }
            Kind::Utf16(endian, d) => d.decode(*endian, input, last, false, out),
            Kind::SingleByte(index) => single_byte::decode(index, input, false, out),
            Kind::XUserDefined => {
                single_byte::decode_x_user_defined(input, out);
                Ok(())
            }
            Kind::Gb18030(d) => d.decode(input, last, false, out),
            Kind::Big5(d) => d.decode(input, last, false, out),
            Kind::EucJp(d) => d.decode(input, last, false, out),
            Kind::Iso2022Jp(d) => d.decode(input, last, false, out),
            Kind::ShiftJis(d) => d.decode(input, last, false, out),
            Kind::EucKr(d) => d.decode(input, last, false, out),
            Kind::Replacement { written } => {
                if !input.is_empty() && !*written {
                    *written = true;
                    out.extend_from_slice(REPLACEMENT);
                }
                Ok(())
            }
        };
        // a decoder that is not fatal writes U+FFFD and goes on
        let Ok(()) = r else { unreachable!("a lossy decoder failed") };
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

/// Append a scalar value as UTF-8.
fn push(out: &mut Vec<u8>, c: u32) {
    let c = char::from_u32(c).expect("the decoders make only scalar values");
    out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
}
