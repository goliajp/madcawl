//! The Encoding Standard's TextDecoder: any encoding the standard names
//! but the replacement one, decoded by the HTML parser's decoders, in
//! the error mode "replacement" or "fatal", in one call or as a stream
//! across calls.

use madcawl_html::{Decoder, Encoding};
use madcawl_js::{Engine, ErrorKind};
use madcawl_str::DomString;

use crate::convert::{Thrown, throw};
use crate::dict::{convert, dictionary};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

pub(crate) struct DecoderState {
    encoding: Encoding,
    fatal: bool,
    ignore_bom: bool,
    decoder: Decoder,
    /// Something was decoded since the stream began: a byte order mark
    /// is no longer looked for.
    bom_seen: bool,
    /// The last call left a stream open.
    do_not_flush: bool,
}

impl DecoderState {
    fn fresh_decoder(&self) -> Decoder {
        if self.fatal { Decoder::fatal(self.encoding) } else { Decoder::new(self.encoding) }
    }
}

fn state<E: Engine>(h: &mut DomHost<E>, r: SideRef) -> &mut DecoderState {
    match &mut h.sides.get_mut(r).data {
        SideData::TextDecoder(d) => d,
        _ => unreachable!("a TextDecoder's side object"),
    }
}

/// A boolean member of a dictionary, false when left out.
fn flag<E: Engine>(e: &mut E, h: &mut DomHost<E>, dict: Option<E::Value>, name: &str) -> Result<bool, Thrown> {
    let Some(d) = dict else { return Ok(false) };
    Ok(convert(e, h, d, name, |e, _, v| Ok(e.to_boolean(v)))?.unwrap_or(false))
}

pub(crate) fn constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, label: DomString, options: E::Value) -> Result<SideRef, Thrown> {
    let dict = dictionary(e, options, "the TextDecoder options")?;
    let fatal = flag(e, h, dict, "fatal")?;
    let ignore_bom = flag(e, h, dict, "ignoreBOM")?;
    let mut b = Vec::new();
    label.write_wtf8(&mut b);
    let encoding = match Encoding::for_label(&b) {
        Some(Encoding::Replacement) | None => return Err(throw(e, ErrorKind::Range, "not a label of a supported encoding")),
        Some(enc) => enc,
    };
    let decoder = if fatal { Decoder::fatal(encoding) } else { Decoder::new(encoding) };
    let s = DecoderState { encoding, fatal, ignore_bom, decoder, bom_seen: false, do_not_flush: false };
    Ok(h.sides.add(SideData::TextDecoder(Box::new(s))))
}

pub(crate) fn encoding<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    Ok(DomString::from_latin1(state(h, this).encoding.name().to_ascii_lowercase().as_bytes()))
}

pub(crate) fn fatal<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<bool, Thrown> {
    Ok(state(h, this).fatal)
}

pub(crate) fn ignore_bom<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<bool, Thrown> {
    Ok(state(h, this).ignore_bom)
}

/// The bytes of a buffer source argument, copied once the arguments are
/// converted: script the options run may change them before.
fn has_bytes<E: Engine>(e: &mut E, h: &DomHost<E>, input: E::Value) -> Result<bool, Thrown> {
    if input == E::undefined() {
        return Ok(false);
    }
    match e.buffer_source(h.realm, input) {
        Some(_) => Ok(true),
        None => Err(throw(e, ErrorKind::Type, "decode takes an ArrayBuffer or a view of one")),
    }
}

pub(crate) fn decode<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, input: E::Value, options: E::Value) -> Result<String, Thrown> {
    let given = has_bytes(e, h, input)?;
    let dict = dictionary(e, options, "the decode options")?;
    let stream = flag(e, h, dict, "stream")?;
    let bytes = if given { e.buffer_source(h.realm, input).map(<[u8]>::to_vec).unwrap_or_default() } else { Vec::new() };
    let s = state(h, this);
    if !s.do_not_flush {
        s.decoder = s.fresh_decoder();
        s.bom_seen = false;
    }
    s.do_not_flush = stream;
    let mut out = Vec::with_capacity(bytes.len());
    if s.decoder.try_decode(&bytes, &mut out, !stream).is_err() {
        // the next call starts a new stream
        s.do_not_flush = false;
        return Err(throw(e, ErrorKind::Type, "the input is not valid in the decoder's encoding"));
    }
    let mut start = 0;
    if !s.bom_seen && !out.is_empty() {
        s.bom_seen = true;
        let unicode = matches!(s.encoding, Encoding::Utf8 | Encoding::Utf16be | Encoding::Utf16le);
        if unicode && !s.ignore_bom && out.starts_with("\u{feff}".as_bytes()) {
            start = 3;
        }
    }
    out.drain(..start);
    Ok(String::from_utf8(out).expect("the decoders make only scalar values"))
}
