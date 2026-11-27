//! The host's reads and definitions through a realm's own builtins
//! (`helpers.rs`).

use madcawl_js::{ArrayType, Builtins, ErrorInfo};
use madcawl_str::DomString;

use crate::ffi;
use crate::helpers::Helper;
use crate::isolate::{Jsc, Realm};
use crate::value::Value;

impl Builtins for Jsc {
    fn typed_array(&mut self, v: Value) -> Option<(ArrayType, &mut [u8])> {
        self.typed_array_bytes(v)
    }

    fn error_info(&mut self, realm: Realm, v: Value, max: usize) -> Option<ErrorInfo> {
        if self.halted() {
            return None;
        }
        let r = self.index(realm);
        let n = Value::number(max.saturating_add(1) as f64);
        let out = match self.call_helper(r, Helper::ErrorInfo, &[v, n]) {
            Ok(out) if out.is_cell() => out,
            Ok(_) => return None,
            Err(_) => {
                self.after_stop();
                return None;
            }
        };
        let ctx = self.realms[r as usize].ctx;
        // SAFETY: the array the helper just returned, on the stack, with
        // its five elements its own
        let at = |i| Value::from_ref(unsafe { ffi::JSObjectGetPropertyAtIndex(ctx, out.0 as _, i, std::ptr::null_mut()) });
        let number = |v: Value| v.as_number().filter(|n| n.is_finite() && *n >= 0.0).map_or(0, |n| n as u32);
        let (name, message, source, line, column) = (at(0), at(1), at(2), at(3), at(4));
        Some(ErrorInfo {
            name: self.string_value(name),
            message: self.string_value(message),
            source: self.string_value(source),
            line: number(line),
            column: number(column),
        })
    }

    fn define_accessor(&mut self, realm: Realm, obj: Value, name: &str, get: Value, set: Value, unforgeable: bool) -> bool {
        let r = self.index(realm);
        let n = self.string_of(&DomString::from_wtf8(name.as_bytes()));
        let u = if unforgeable { crate::value::TRUE } else { crate::value::FALSE };
        let ok = self.call_helper(r, Helper::Accessor, &[obj, n, get, set, u]).is_ok();
        self.unprotect(n);
        if !ok {
            self.after_stop();
        }
        ok
    }
}
