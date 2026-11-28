//! The XMLHttpRequest interface's members (`xhr.rs` has the request
//! and its events, `xhr_http.rs` the checks on names and values).

use madcawl_html::{Decoder, Encoding};
use madcawl_js::{Engine, ErrorKind, Member};
use madcawl_str::{DomString, atom};

use crate::convert::{Any, Thrown, plain, throw, throw_dom};
use crate::event::Target;
use crate::host::DomHost;
use crate::side::{SideData, SideRef};
use crate::xhr::{DONE, HEADERS_RECEIVED, Kind, LOADING, OPENED, UNSENT, XhrState};
use crate::xhr_http::{all_headers, header, is_forbidden_header, is_forbidden_method, is_token, is_value, mime, normalize, normalize_method};

/// A ByteString: each code unit a byte, or a TypeError.
fn byte_string<E: Engine>(e: &mut E, s: &DomString) -> Result<Vec<u8>, Thrown> {
    s.units().map(|u| u8::try_from(u).ok()).collect::<Option<Vec<u8>>>().ok_or_else(|| throw(e, ErrorKind::Type, "a code unit above U+00FF in a byte string"))
}

pub(crate) fn constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>) -> Result<SideRef, Thrown> {
    let realm = h.caller_realm(e)?;
    let w = h.window_of_realm(realm).expect("a live window's realm");
    Ok(h.sides.add(SideData::Xhr(Box::new(XhrState::new(w)))))
}

pub(crate) fn onreadystatechange<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
    Ok(Any(h.event_handler(e, Target::Side(this), atom!("readystatechange"))))
}

pub(crate) fn set_onreadystatechange<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, value: Option<E::Value>) -> Result<(), Thrown> {
    h.set_event_handler(e, Target::Side(this), atom!("readystatechange"), value);
    Ok(())
}

pub(crate) fn ready_state<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<u16, Thrown> {
    Ok(h.xhr(this).state)
}

pub(crate) fn open<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    this: SideRef,
    method: DomString,
    url: String,
    r#async: E::Value,
) -> Result<(), Thrown> {
    let m = byte_string(e, &method)?;
    if !is_token(&m) {
        return Err(throw_dom(e, h, "SyntaxError", "the method is not a token"));
    }
    let m = String::from_utf8(m).expect("a token is ASCII");
    if is_forbidden_method(&m) {
        return Err(throw_dom(e, h, "SecurityError", "a forbidden method"));
    }
    let w = h.xhr(this).window;
    let doc = h.doc_of_window(w).expect("an XMLHttpRequest's window outlives its script");
    let Ok(parsed) = h.base_url(doc).join(&url) else { return Err(throw_dom(e, h, "SyntaxError", "the URL does not parse")) };
    if r#async != E::undefined() && !e.to_boolean(r#async) {
        return Err(throw_dom(e, h, "InvalidAccessError", "madcawl makes asynchronous requests only"));
    }
    h.xhr_reset(e, this);
    let x = h.xhr_mut(this);
    (x.send, x.method, x.url) = (false, normalize_method(&m), Some(parsed));
    x.headers.clear();
    if x.state != OPENED {
        x.state = OPENED;
        h.fire_simple(e, Target::Side(this), atom!("readystatechange"), false);
    }
    Ok(())
}

pub(crate) fn set_request_header<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, name: DomString, value: DomString) -> Result<(), Thrown> {
    let (n, v) = (byte_string(e, &name)?, byte_string(e, &value)?);
    let x = h.xhr(this);
    if x.state != OPENED || x.send {
        return Err(throw_dom(e, h, "InvalidStateError", "the request is not opened or was sent"));
    }
    let v = normalize(&v);
    if !is_token(&n) || !is_value(v) {
        return Err(throw_dom(e, h, "SyntaxError", "not a header name and value"));
    }
    let n = String::from_utf8(n).expect("a token is ASCII");
    if is_forbidden_header(&n, v) {
        return Ok(());
    }
    let x = h.xhr_mut(this);
    match x.headers.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(&n)) {
        Some((_, old)) => {
            old.extend_from_slice(b", ");
            old.extend_from_slice(v);
        }
        None => x.headers.push((n, v.to_vec())),
    }
    Ok(())
}

pub(crate) fn timeout<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<u32, Thrown> {
    Ok(h.xhr(this).timeout)
}

/// A new timeout counts from `send`, also while the request waits; its
/// timer replaces the one there was.
pub(crate) fn set_timeout<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, value: u32) -> Result<(), Thrown> {
    let x = h.xhr_mut(this);
    x.timeout = value;
    if x.request.is_none() {
        return Ok(());
    }
    if value == 0 {
        h.xhr_clear_timer(e, this);
        return Ok(());
    }
    let due = x.sent_ns.saturating_add(u64::from(value) * 1_000_000);
    let ms = due.saturating_sub(h.event_loop.now_ns()).div_ceil(1_000_000);
    h.xhr_timer(e, this, ms);
    Ok(())
}

pub(crate) fn with_credentials<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<bool, Thrown> {
    Ok(h.xhr(this).credentials)
}

pub(crate) fn set_with_credentials<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, value: bool) -> Result<(), Thrown> {
    let x = h.xhr(this);
    if !matches!(x.state, UNSENT | OPENED) || x.send {
        return Err(throw_dom(e, h, "InvalidStateError", "the request was sent"));
    }
    h.xhr_mut(this).credentials = value;
    Ok(())
}

pub(crate) fn send<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, body: E::Value) -> Result<(), Thrown> {
    let x = h.xhr(this);
    if x.state != OPENED || x.send {
        return Err(throw_dom(e, h, "InvalidStateError", "the request is not opened or was sent"));
    }
    let (body, ty) = match x.method.as_str() {
        "GET" | "HEAD" => (None, None),
        _ => super::xml_http_request_body::extract(e, h, body)?,
    };
    h.xhr_start(e, this, body, ty);
    Ok(())
}

pub(crate) fn abort<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<(), Thrown> {
    let x = h.xhr(this);
    if (x.state == OPENED && x.send) || matches!(x.state, HEADERS_RECEIVED | LOADING) {
        h.xhr_error(e, this, atom!("abort"));
    } else {
        h.xhr_reset(e, this);
    }
    let x = h.xhr_mut(this);
    if x.state == DONE {
        (x.state, x.response) = (UNSENT, None);
    }
    Ok(())
}

pub(crate) fn response_url<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    let url = h.xhr(this).response.as_ref().map_or("", |r| r.url.as_str());
    Ok(url[..url.find('#').unwrap_or(url.len())].to_owned())
}

pub(crate) fn status<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<u16, Thrown> {
    Ok(h.xhr(this).response.as_ref().map_or(0, |r| r.status))
}

pub(crate) fn status_text<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    Ok(DomString::from_latin1(h.xhr(this).response.as_ref().map_or(&[][..], |r| &r.status_text)))
}

pub(crate) fn get_response_header<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, name: DomString) -> Result<Option<DomString>, Thrown> {
    let n = byte_string(e, &name)?;
    let Some(r) = h.xhr(this).response.as_ref() else { return Ok(None) };
    Ok(header(&r.headers, &String::from_utf8_lossy(&n)).map(|v| DomString::from_latin1(&v)))
}

pub(crate) fn get_all_response_headers<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    Ok(DomString::from_latin1(&h.xhr(this).response.as_ref().map_or_else(Vec::new, |r| all_headers(&r.headers))))
}

/// The MIME type the response is read as: the overriding one, or its
/// `Content-Type`.
fn final_mime<E: Engine>(h: &DomHost<E>, this: SideRef) -> String {
    let x = h.xhr(this);
    let own = x.response.as_ref().and_then(|r| header(&r.headers, "content-type")).map(|v| String::from_utf8_lossy(&v).into_owned());
    x.mime.clone().or(own).unwrap_or_default()
}

pub(crate) fn override_mime_type<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, value: DomString) -> Result<(), Thrown> {
    if matches!(h.xhr(this).state, LOADING | DONE) {
        return Err(throw_dom(e, h, "InvalidStateError", "the response is loading or loaded"));
    }
    let s = String::from_utf8_lossy(&super::event::wtf8(&value)).into_owned();
    let valid = mime(&s).0.split_once('/').is_some_and(|(t, st)| crate::xhr_http::is_token(t.as_bytes()) && crate::xhr_http::is_token(st.as_bytes()));
    h.xhr_mut(this).mime = Some(if valid { s } else { "application/octet-stream".to_owned() });
    Ok(())
}

const KINDS: [(&str, Kind); 6] = [("", Kind::Default), ("arraybuffer", Kind::ArrayBuffer), ("blob", Kind::Blob), ("document", Kind::Document), ("json", Kind::Json), ("text", Kind::Text)];

pub(crate) fn response_type<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    let k = h.xhr(this).kind;
    Ok(DomString::from_latin1(KINDS.iter().find(|(_, x)| *x == k).expect("a listed kind").0.as_bytes()))
}

/// A value that is not one of the enumeration's is ignored.
pub(crate) fn set_response_type<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, value: DomString) -> Result<(), Thrown> {
    let Some(&(_, k)) = KINDS.iter().find(|(n, _)| value.eq_str(n)) else { return Ok(()) };
    if matches!(h.xhr(this).state, LOADING | DONE) {
        return Err(throw_dom(e, h, "InvalidStateError", "the response is loading or loaded"));
    }
    h.xhr_mut(this).kind = k;
    Ok(())
}

/// The body as text: decoded by the charset of its MIME type, UTF-8 by
/// default, a byte order mark first deciding.
fn text<E: Engine>(h: &DomHost<E>, this: SideRef) -> DomString {
    let x = h.xhr(this);
    let Some(r) = x.response.as_ref().filter(|_| matches!(x.state, LOADING | DONE)) else { return DomString::empty() };
    let label = mime(&final_mime(h, this)).1;
    let (body, enc) = match &r.body[..] {
        [0xEF, 0xBB, 0xBF, rest @ ..] => (rest, Encoding::Utf8),
        b @ [0xFE, 0xFF, ..] => (&b[2..], Encoding::Utf16be),
        b @ [0xFF, 0xFE, ..] => (&b[2..], Encoding::Utf16le),
        b => (b, label.and_then(|l| Encoding::for_label(l.as_bytes())).filter(|e| *e != Encoding::Replacement).unwrap_or(Encoding::Utf8)),
    };
    let mut out = Vec::with_capacity(body.len());
    Decoder::new(enc).decode(body, &mut out, true);
    DomString::from_wtf8(&out)
}

pub(crate) fn response_text<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    if !matches!(h.xhr(this).kind, Kind::Default | Kind::Text) {
        return Err(throw_dom(e, h, "InvalidStateError", "the response type is not text"));
    }
    Ok(text(h, this))
}

/// The response as its type has it, made once and kept as the
/// XMLHttpRequest's member; null until it is done, or with no response.
pub(crate) fn response<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
    let x = h.xhr(this);
    if matches!(x.kind, Kind::Default | Kind::Text) {
        let s = text(h, this);
        return Ok(Any(e.string(s)));
    }
    if x.state != DONE || x.response.is_none() || x.kind == Kind::Document {
        return Ok(Any(plain::<E>(E::null())));
    }
    if let Some(v) = h.details.get(&this).and_then(Member::get) {
        return Ok(Any(e.retain(v)));
    }
    let (kind, body) = (x.kind, x.response.as_ref().expect("checked").body.clone());
    let realm = h.caller_realm(e)?;
    let v = match kind {
        Kind::ArrayBuffer => e.new_bytes(realm, body.to_vec(), true),
        Kind::Blob => {
            // a Blob's type is lowercased, and empty with a byte outside U+0020 to U+007E
            let ty = final_mime(h, this);
            let ty = if ty.bytes().all(|b| (0x20..=0x7e).contains(&b)) { ty.to_ascii_lowercase() } else { String::new() };
            let b = super::blob::new_blob(h, body, ty);
            h.wrap_side(e, b)
        }
        _ => {
            let s = super::blob::utf8_decode(&body);
            e.parse_json(realm, &DomString::from_wtf8(s.as_bytes())).unwrap_or_else(|| plain::<E>(E::null()))
        }
    };
    if E::is_thrown(v.get()) {
        let _sentinel = v.into_raw();
        return Err(Thrown);
    }
    h.set_value(e, this, v.get());
    Ok(Any(v))
}
