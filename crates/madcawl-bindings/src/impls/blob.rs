//! The File API's Blob: immutable bytes and a type. A slice shares its
//! blob's bytes. `text()` and `arrayBuffer()` settle their promise in a
//! task, as reading the bytes in parallel would. A buffer source part is
//! copied as it is converted, before the options are read.

use std::ops::Range;
use std::rc::Rc;

use madcawl_js::{Engine, ErrorKind};
use madcawl_str::DomString;

use crate::convert::{Any, Thrown, throw, usv_string};
use crate::dict::{Flow, convert, dictionary, for_each_item, is_object, iterator_method};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

/// The most bytes one blob holds.
pub(crate) const MAX_BYTES: usize = 1 << 28;

#[derive(Clone)]
pub(crate) struct BlobData {
    bytes: Rc<[u8]>,
    range: Range<usize>,
    pub ty: String,
}

impl BlobData {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes[self.range.clone()]
    }
}

fn data<E: Engine>(h: &DomHost<E>, r: SideRef) -> &BlobData {
    match &h.sides.get(r).data {
        SideData::Blob(b) => b,
        _ => unreachable!("a Blob's side object"),
    }
}

/// The blob `v` is a wrapper of, if it is one.
pub(crate) fn blob_of<E: Engine>(e: &E, h: &DomHost<E>, v: E::Value) -> Option<BlobData> {
    let r = e.host_of(v, &crate::generated::BlobClass::<E>::DEF).and_then(|o| h.sides.of(o))?;
    Some(data(h, r).clone())
}

/// A type as the standard keeps it: lowercased, or empty when it has a
/// character outside U+0020 to U+007E.
fn normal_type(t: &DomString) -> String {
    match t.as_ascii_str() {
        Some(a) if a.bytes().all(|b| (0x20..=0x7e).contains(&b)) => a.to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// A part, converted: bytes, or a string still to have its line
/// endings converted.
enum Part {
    Bytes(Vec<u8>),
    Text(String),
}

fn part<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value) -> Result<Part, Thrown> {
    if let Some(b) = blob_of(e, h, v) {
        return Ok(Part::Bytes(b.bytes().to_vec()));
    }
    if let Some(b) = e.buffer_source(h.realm, v) {
        return Ok(Part::Bytes(b.to_vec()));
    }
    usv_string(e, h, v).map(Part::Text)
}

fn too_large<E: Engine>(e: &mut E) -> Thrown {
    throw(e, ErrorKind::Range, "the blob is larger than madcawl allows")
}

pub(crate) fn constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, parts: E::Value, options: E::Value) -> Result<SideRef, Thrown> {
    let mut list = Vec::new();
    if parts != E::undefined() {
        let method = if is_object(e, parts) { iterator_method(e, h, parts)? } else { None };
        let Some(method) = method else { return Err(throw(e, ErrorKind::Type, "the blob parts are not a sequence")) };
        let mut total = 0usize;
        let r = for_each_item(e, h, parts, method.get(), |e, h, item| {
            let p = part(e, h, item)?;
            total += match &p {
                Part::Bytes(b) => b.len(),
                Part::Text(s) => s.len(),
            };
            if total > MAX_BYTES {
                return Err(too_large(e));
            }
            list.push(p);
            Ok(Flow::Next)
        });
        e.release(method);
        r?;
    }
    let dict = dictionary(e, options, "the blob options")?;
    let native = match dict {
        Some(d) => match convert(e, h, d, "endings", |e, h, v| e.to_string(h, v).ok_or(Thrown))? {
            None => false,
            Some(s) if s.eq_str("transparent") => false,
            Some(s) if s.eq_str("native") => true,
            Some(_) => return Err(throw(e, ErrorKind::Type, "endings is neither transparent nor native")),
        },
        None => false,
    };
    let ty = match dict {
        Some(d) => convert(e, h, d, "type", |e, h, v| e.to_string(h, v).ok_or(Thrown))?.map(|t| normal_type(&t)).unwrap_or_default(),
        None => String::new(),
    };
    let mut bytes = Vec::new();
    for p in list {
        match p {
            Part::Bytes(b) => bytes.extend_from_slice(&b),
            Part::Text(s) if native => bytes.extend_from_slice(s.replace("\r\n", "\n").replace('\r', "\n").as_bytes()),
            Part::Text(s) => bytes.extend_from_slice(s.as_bytes()),
        }
    }
    Ok(new_blob(h, bytes.into(), ty))
}

fn new_blob<E: Engine>(h: &mut DomHost<E>, bytes: Rc<[u8]>, ty: String) -> SideRef {
    let range = 0..bytes.len();
    h.sides.add(SideData::Blob(BlobData { bytes, range, ty }))
}

pub(crate) fn size<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<f64, Thrown> {
    Ok(data(h, this).range.len() as f64)
}

pub(crate) fn r#type<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    Ok(DomString::from_latin1(data(h, this).ty.as_bytes()))
}

/// A `[Clamp] long long` position, made relative to a blob of `size`
/// bytes: from the end when negative, kept within the blob.
fn position<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value, size: usize, default: usize) -> Result<usize, Thrown> {
    if v == E::undefined() {
        return Ok(default);
    }
    let n = e.to_number(h, v).ok_or(Thrown)?;
    let n = if n.is_nan() { 0.0 } else { n.clamp(-(2f64.powi(63)), 2f64.powi(63) - 1.0).round_ties_even() };
    let size = size as f64;
    Ok(if n < 0.0 { (size + n).max(0.0) } else { n.min(size) } as usize)
}

pub(crate) fn slice<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, start: E::Value, end: E::Value, ty: E::Value) -> Result<SideRef, Thrown> {
    let size = data(h, this).range.len();
    let start = position(e, h, start, size, 0)?;
    let end = position(e, h, end, size, size)?;
    let ty = if ty == E::undefined() { String::new() } else { normal_type(&e.to_string(h, ty).ok_or(Thrown)?) };
    let mut b = data(h, this).clone();
    let from = b.range.start + start;
    b.range = from..from + end.saturating_sub(start);
    b.ty = ty;
    Ok(h.sides.add(SideData::Blob(b)))
}

/// A promise of `realm`, the caller's, resolved with `value`, in a task.
fn settle_later<E: Engine>(e: &mut E, h: &mut DomHost<E>, realm: E::Realm, value: madcawl_js::Owned<E::Value>) -> Result<Any<E>, Thrown> {
    if E::is_thrown(value.get()) {
        let _sentinel = value.into_raw();
        return Err(Thrown);
    }
    let Some(d) = e.new_promise(realm) else {
        e.release(value);
        return Err(Thrown);
    };
    e.release(d.reject);
    h.post_settle(realm, d.resolve, value);
    Ok(Any(d.promise))
}

/// UTF-8 decode: a leading byte order mark dropped, errors as U+FFFD.
pub(crate) fn utf8_decode(b: &[u8]) -> String {
    String::from_utf8_lossy(b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b)).into_owned()
}

pub(crate) fn text<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
    let realm = h.caller_realm(e)?;
    let s = utf8_decode(data(h, this).bytes());
    let v = e.string(DomString::from_wtf8(s.as_bytes()));
    settle_later(e, h, realm, v)
}

pub(crate) fn array_buffer<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
    let realm = h.caller_realm(e)?;
    let b = data(h, this).bytes().to_vec();
    let v = e.new_bytes(realm, b, true);
    settle_later(e, h, realm, v)
}

#[cfg(test)]
mod tests {
    #[test]
    fn utf8_decode_drops_one_byte_order_mark_and_replaces_errors() {
        assert_eq!(super::utf8_decode(b"\xEF\xBB\xBF\xEF\xBB\xBFa"), "\u{feff}a");
        assert_eq!(super::utf8_decode(b"a\xE2\x82b\xFF"), "a\u{fffd}b\u{fffd}");
    }
}
