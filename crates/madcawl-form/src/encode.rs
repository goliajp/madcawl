//! The three encodings of an entry list (HTML 4.10.21.7 - 4.10.21.9):
//! `application/x-www-form-urlencoded`, `multipart/form-data` (RFC 7578)
//! and `text/plain`, all in UTF-8. Names and text values have their
//! newlines made CRLF first.

use crate::{Entry, FormError, Value};
use std::io::Read;
use std::path::Path;

/// Every lone CR or LF as CRLF.
pub fn normalize_newlines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                out.push_str("\r\n");
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            }
            '\n' => out.push_str("\r\n"),
            c => out.push(c),
        }
    }
    out
}

/// The entry list as name-value pairs: newlines normalized, a file
/// standing for its name.
pub fn pairs(entries: &[Entry]) -> Vec<(String, String)> {
    let value = |v: &Value| match v {
        Value::Text(t) => normalize_newlines(t),
        Value::File(f) => f.filename.clone(),
    };
    entries.iter().map(|e| (normalize_newlines(&e.name), value(&e.value))).collect()
}

pub fn urlencoded(entries: &[Entry]) -> String {
    let mut out = String::new();
    for (i, (n, v)) in pairs(entries).iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        alt_url::form::append_pair(&mut out, n, v);
    }
    out
}

pub fn text_plain(entries: &[Entry]) -> String {
    let mut out = String::new();
    for (n, v) in pairs(entries) {
        out.push_str(&n);
        out.push('=');
        out.push_str(&v);
        out.push_str("\r\n");
    }
    out
}

/// A name or file name inside a quoted header parameter.
fn escape_param(s: &str) -> String {
    s.replace('\n', "%0A").replace('\r', "%0D").replace('"', "%22")
}

/// A boundary of 22 letters and digits that the process's hasher seeds
/// make unpredictable to a page.
pub fn boundary() -> String {
    use std::hash::{BuildHasher, RandomState};
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let (a, b) = (RandomState::new().hash_one(1u8), RandomState::new().hash_one(2u8));
    let mut out = String::from("----madcawl");
    let mut bits = u128::from(a) << 64 | u128::from(b);
    for _ in 0..22 {
        out.push(char::from(ALPHABET[(bits % 62) as usize]));
        bits /= 62;
    }
    out
}

/// What the encoder may read: a file is read only if `allow` says yes,
/// and the whole body stays within `max_body` bytes.
pub struct Files<'a> {
    pub allow: &'a dyn Fn(&Path) -> bool,
    pub max_body: usize,
}

fn append_file(out: &mut Vec<u8>, path: &Path, files: &Files) -> Result<(), FormError> {
    if !(files.allow)(path) {
        return Err(FormError::FileNotAllowed(path.to_owned()));
    }
    let io = |e: std::io::Error| FormError::File(path.to_owned(), e.to_string());
    let room = files.max_body.saturating_sub(out.len());
    let n = std::fs::File::open(path).map_err(io)?.take(room as u64 + 1).read_to_end(out).map_err(io)?;
    if n > room {
        return Err(FormError::TooLarge);
    }
    Ok(())
}

/// The multipart body with `boundary` between the parts.
pub fn multipart(entries: &[Entry], boundary: &str, files: &Files) -> Result<Vec<u8>, FormError> {
    let mut out = Vec::new();
    for e in entries {
        let name = escape_param(&normalize_newlines(&e.name));
        out.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"").as_bytes());
        match &e.value {
            Value::Text(t) => {
                out.extend_from_slice(b"\r\n\r\n");
                out.extend_from_slice(normalize_newlines(t).as_bytes());
            }
            Value::File(f) => {
                let ty: String = f.content_type.chars().filter(|c| !c.is_control()).collect();
                let ty = if ty.is_empty() { "application/octet-stream" } else { ty.as_str() };
                let head = format!("; filename=\"{}\"\r\nContent-Type: {ty}\r\n\r\n", escape_param(&f.filename));
                out.extend_from_slice(head.as_bytes());
                if let Some(p) = &f.path {
                    append_file(&mut out, p, files)?;
                }
            }
        }
        out.extend_from_slice(b"\r\n");
        if out.len() > files.max_body {
            return Err(FormError::TooLarge);
        }
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok(out)
}
