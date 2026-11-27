//! The exceptions nothing caught, as the host reports them: each its
//! message and where it was thrown. An error object's own data
//! properties are read through the engine's builtins and a
//! DOMException's from its host object, so reading them runs no page
//! code: no getter, no proxy trap, no `toString`. (JavaScriptCore itself
//! still calls `toString` on an exception that leaves a C API call.)
//! Any other object is reported as one, unread.

use std::fmt;

use madcawl_js::{Engine, ErrorInfo, ValueKind};
use madcawl_str::DomString;

use crate::generated::DOMExceptionClass;
use crate::host::DomHost;
use crate::notes::Note;
use crate::side::SideData;

/// The longest message kept, in UTF-16 code units.
const MAX_MESSAGE: usize = 512;
/// The longest source name kept, in UTF-16 code units.
const MAX_SOURCE: usize = 256;

/// An exception script did not catch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    /// `name: message` of an error object, or the value thrown
    pub message: String,
    /// The script's URL as the host named it; empty when unknown
    pub source: String,
    /// 1-based; 0 when unknown
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if !self.source.is_empty() || self.line != 0 {
            write!(f, " @{}:{}:{}", self.source, self.line, self.column)?;
        }
        Ok(())
    }
}

/// The first `max` code units of `s` on one line, with an ellipsis
/// when there were more; only those are converted.
fn text(s: &DomString, max: usize) -> String {
    let (mut out, mut units) = (String::new(), 0);
    for c in char::decode_utf16(s.units()) {
        let c = c.unwrap_or(char::REPLACEMENT_CHARACTER);
        units += if c == char::REPLACEMENT_CHARACTER { 1 } else { c.len_utf16() };
        if units > max {
            out.push('…');
            break;
        }
        out.push(if c.is_control() { ' ' } else { c });
    }
    out
}

/// `name: message` as an error's own `toString` would put it.
fn joined(name: Option<&DomString>, message: Option<&DomString>) -> String {
    match (name, message.filter(|m| !m.is_empty())) {
        (Some(n), Some(m)) => format!("{}: {}", text(n, MAX_MESSAGE), text(m, MAX_MESSAGE)),
        (Some(n), None) => text(n, MAX_MESSAGE),
        (None, Some(m)) => text(m, MAX_MESSAGE),
        (None, None) => "an error".to_owned(),
    }
}

impl<E: Engine> DomHost<E> {
    pub(crate) fn record_exception(&mut self, e: &mut E, x: E::Value) {
        let err = self.describe(e, x);
        self.note(Note::Exception(err));
    }

    fn describe(&mut self, e: &mut E, x: E::Value) -> ScriptError {
        let mut err = ScriptError { message: String::new(), source: String::new(), line: 0, column: 0 };
        let side = e.host_of(x, &DOMExceptionClass::<E>::DEF).and_then(|obj| self.sides.of(obj));
        if let Some(SideData::DomException(d)) = side.map(|r| &self.sides.get(r).data) {
            err.message = joined(Some(&d.name), Some(&d.message));
        } else if let Some(info) = e.error_info(self.realm, x, MAX_MESSAGE) {
            let ErrorInfo { name, message, source, line, column } = info;
            err.message = match e.kind(x) {
                ValueKind::String => message.map_or_else(String::new, |m| text(&m, MAX_MESSAGE)),
                _ => joined(name.as_ref(), message.as_ref()),
            };
            err.source = source.map_or_else(String::new, |s| text(&s, MAX_SOURCE));
            (err.line, err.column) = (line, column);
        } else {
            err.message = self.primitive(e, x);
        }
        err
    }

    /// A value that is no error object, as text: a primitive's ToString,
    /// which runs no script.
    fn primitive(&mut self, e: &mut E, x: E::Value) -> String {
        match e.kind(x) {
            ValueKind::Object | ValueKind::Function => "an object".to_owned(),
            ValueKind::Symbol => "a symbol".to_owned(),
            ValueKind::String => e.as_string(x).map_or_else(String::new, |s| text(&s, MAX_MESSAGE)),
            _ => e.to_string(self, x).map_or_else(String::new, |s| text(&s, MAX_MESSAGE)),
        }
    }
}

#[cfg(test)]
mod tests {
    use madcawl_str::DomString;

    use super::{ScriptError, text};

    fn t(s: &str, max: usize) -> String {
        text(&DomString::from_wtf8(s.as_bytes()), max)
    }

    #[test]
    fn a_message_is_cut_to_one_line_of_its_length() {
        assert_eq!(t("a\nb", 10), "a b");
        assert_eq!(t("abcdef", 3), "abc…");
        assert_eq!(t("abc", 3), "abc");
        assert_eq!(t("é😀x", 3), "é😀…", "a surrogate pair is two code units");
    }

    #[test]
    fn an_error_shows_where_it_was_thrown_when_known() {
        let mut e = ScriptError { message: "TypeError: x".into(), source: "inline".into(), line: 1, column: 7 };
        assert_eq!(e.to_string(), "TypeError: x @inline:1:7");
        (e.source, e.line) = (String::new(), 0);
        assert_eq!(e.to_string(), "TypeError: x");
    }
}
