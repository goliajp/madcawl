//! Strings that are not a flat block: a substring view, as an engine
//! keeps one to avoid a copy. The host gets a flat block for it from
//! `as_string`, made anew.

use madcawl_js::Owned;
use madcawl_str::DomString;

use crate::cell::{Body, Cell};
use crate::isolate::{Stub, string_at};
use crate::value::Value;

impl Stub {
    /// A view of `len` code units of flat string `s` from `start`; it
    /// holds `s`.
    pub fn substring(&mut self, s: Value, start: u32, len: u32) -> Owned<Value> {
        assert!(s.is_string(), "a view into a flat string");
        // SAFETY: borrowed; the value keeps its count
        let n = std::mem::ManuallyDrop::new(unsafe { string_at(s) }).len();
        assert!(start as usize + len as usize <= n, "a view inside its string");
        self.retain_value(s);
        Stub::owned(self.alloc(self.current, Body::StrView { s, start, len }))
    }

    pub(crate) fn is_view(&self, v: Value) -> bool {
        // SAFETY: a live cell, read only
        v.is_cell() && matches!(unsafe { Cell::at(v) }.body, Body::StrView { .. })
    }

    /// The length of string `v` in code units.
    pub(crate) fn string_len(&self, v: Value) -> Option<usize> {
        if v.is_string() {
            // SAFETY: borrowed; the value keeps its count
            return Some(std::mem::ManuallyDrop::new(unsafe { string_at(v) }).len());
        }
        // SAFETY: a live cell, read only
        match v.is_cell().then(|| &unsafe { Cell::at(v) }.body) {
            Some(Body::StrView { len, .. }) => Some(*len as usize),
            _ => None,
        }
    }

    /// String `v` as a flat block with a count: the same block, or a
    /// copy of a view's units.
    pub(crate) fn flat_string(&mut self, v: Value) -> Option<DomString> {
        if v.is_string() {
            // SAFETY: borrowed for the clone; the value keeps its count
            let s = std::mem::ManuallyDrop::new(unsafe { string_at(v) });
            return Some(DomString::clone(&s));
        }
        if !self.is_view(v) {
            return None;
        }
        // SAFETY: a live cell, read only
        let Body::StrView { s, start, len } = unsafe { Cell::at(v) }.body else { unreachable!() };
        // SAFETY: the view holds a count on its string
        let whole = std::mem::ManuallyDrop::new(unsafe { string_at(s) });
        let units: Vec<u16> = whole.units().skip(start as usize).take(len as usize).collect();
        Some(DomString::from_utf16(&units))
    }
}
