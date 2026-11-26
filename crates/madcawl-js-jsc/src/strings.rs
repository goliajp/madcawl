//! Strings across the boundary. The framework's strings are its own:
//! a `DomString` is copied in as UTF-16 (Latin-1 widened) and copied
//! out, once each way. Property names that are static atoms are made
//! once per isolate and kept.

use madcawl_js::Key;
use madcawl_str::{Atom, DomString};

use crate::ffi::{self, JSStringRef};
use crate::isolate::Jsc;
use crate::value::Value;

/// A framework string the caller releases.
pub(crate) struct JsStr {
    pub s: JSStringRef,
    owned: bool,
}

impl Drop for JsStr {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: made by this module and not released elsewhere
            unsafe { ffi::JSStringRelease(self.s) };
        }
    }
}

impl Jsc {
    pub(crate) fn js_str(&mut self, s: &DomString) -> JsStr {
        let mut buf = std::mem::take(&mut self.scratch);
        buf.clear();
        match s.as_latin1() {
            Some(b) => buf.extend(b.iter().map(|&c| u16::from(c))),
            None => buf.extend(s.units()),
        }
        // SAFETY: `buf` holds `len` units for the call
        let js = unsafe { ffi::JSStringCreateWithCharacters(buf.as_ptr(), buf.len()) };
        self.scratch = buf;
        JsStr { s: js, owned: true }
    }

    pub(crate) fn js_str_utf8(&mut self, s: &str) -> JsStr {
        let c = std::ffi::CString::new(s).expect("no NUL in an adapter string");
        // SAFETY: a NUL-terminated string
        JsStr { s: unsafe { ffi::JSStringCreateWithUTF8CString(c.as_ptr()) }, owned: true }
    }

    /// The name `a` spells as a framework string.
    pub(crate) fn atom_str(&mut self, a: Atom) -> JsStr {
        if !a.is_static() {
            return self.js_str(&a.to_dom_string());
        }
        if let Some(&s) = self.atom_strings.get(&a.index()) {
            return JsStr { s, owned: false };
        }
        let s = self.js_str(&a.to_dom_string());
        let keep = JsStr { s: s.s, owned: false };
        std::mem::forget(s);
        self.atom_strings.insert(a.index(), keep.s);
        keep
    }

    /// A property name: `None` for an index.
    pub(crate) fn key_str(&mut self, key: Key<'_>) -> Result<JsStr, u32> {
        match key {
            Key::Name(a) => Ok(self.atom_str(a)),
            Key::Index(i) => Err(i),
            Key::Str(s) => match index_of(s) {
                Some(i) => Err(i),
                None => Ok(self.js_str(s)),
            },
        }
    }

    /// A framework string as a `DomString`.
    pub(crate) fn dom_string(s: JSStringRef) -> DomString {
        // SAFETY: a live string; the pointer is valid for its length
        unsafe {
            let n = ffi::JSStringGetLength(s);
            if n == 0 {
                return DomString::empty();
            }
            DomString::from_utf16(std::slice::from_raw_parts(ffi::JSStringGetCharactersPtr(s), n))
        }
    }

    /// The string value `v` is, copied out; `None` when it is not a
    /// string.
    pub(crate) fn string_value(&mut self, v: Value) -> Option<DomString> {
        if !v.is_cell() {
            return None;
        }
        // SAFETY: a live value; a string converts without running script
        unsafe {
            if ffi::JSValueGetType(self.base, v.raw()) != ffi::kJSTypeString {
                return None;
            }
            let s = ffi::JSValueToStringCopy(self.base, v.raw(), std::ptr::null_mut());
            let out = Jsc::dom_string(s);
            ffi::JSStringRelease(s);
            Some(out)
        }
    }

    /// `s` as a string value with a count.
    pub(crate) fn string_of(&mut self, s: &DomString) -> Value {
        let js = self.js_str(s);
        // SAFETY: a live string; the value takes its own reference
        let v = Value::from_ref(unsafe { ffi::JSValueMakeString(self.base, js.s) });
        self.protect(v);
        v
    }
}

/// The array index a property name spells, as the engine reads one.
fn index_of(s: &DomString) -> Option<u32> {
    let b = s.as_latin1()?;
    if b.is_empty() || b.len() > 10 || (b.len() > 1 && b[0] == b'0') || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n: u64 = std::str::from_utf8(b).ok()?.parse().ok()?;
    (n < u64::from(u32::MAX)).then_some(n as u32)
}
