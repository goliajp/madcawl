//! The host's reads and definitions through a realm's own builtins
//! (`helpers.rs`).

use madcawl_js::{ArrayType, Builtins, Deferred, Engine, ErrorInfo, Host, Owned};
use madcawl_str::DomString;

use crate::ffi;
use crate::helpers::Helper;
use crate::isolate::{Jsc, Realm};
use crate::value::{UNDEFINED, Value};

impl Builtins for Jsc {
    fn current_realm(&self) -> Option<Realm> {
        self.running_realm()
            .map(|r| Realm(r, self.id, self.realms[r as usize].generation))
    }

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

    fn iterator_method<H: Host<Jsc>>(&mut self, host: &mut H, realm: Realm, v: Value) -> Owned<Value> {
        let f = self.helper(self.index(realm), Helper::IteratorOf);
        self.call_value(host, f, UNDEFINED, &[v])
    }

    fn array_values<H: Host<Jsc>>(
        &mut self,
        host: &mut H,
        realm: Realm,
        arr: Value,
    ) -> Owned<Value> {
        let f = self.helper(self.index(realm), Helper::ArrayValues);
        self.call_value(host, f, UNDEFINED, &[arr])
    }

    fn buffer_source(&mut self, realm: Realm, v: Value) -> Option<&[u8]> {
        let r = self.index(realm);
        self.buffer_bytes(r, v)
    }

    fn new_bytes(&mut self, realm: Realm, bytes: Vec<u8>, buffer: bool) -> Owned<Value> {
        self.make_bytes(realm, bytes, buffer)
    }

    fn new_promise(&mut self, realm: Realm) -> Option<Deferred<Value>> {
        self.make_promise(realm)
    }

    fn own_entries<H: Host<Jsc>>(&mut self, host: &mut H, realm: Realm, v: Value) -> Owned<Value> {
        let f = self.helper(self.index(realm), Helper::OwnEntries);
        self.call_value(host, f, UNDEFINED, &[v])
    }

    fn parse_json(&mut self, realm: Realm, text: &DomString) -> Option<Owned<Value>> {
        if self.halted() {
            return None;
        }
        let r = self.index(realm);
        let s = self.string_of(text);
        let out = self.call_helper(r, Helper::Json, &[s]);
        self.unprotect(s);
        let out = match out {
            Ok(out) if out.is_cell() => out,
            Ok(_) => return None,
            Err(_) => {
                self.after_stop();
                return None;
            }
        };
        let ctx = self.realms[r as usize].ctx;
        // SAFETY: the array of one the helper just returned, on the stack
        let v = Value::from_ref(unsafe { ffi::JSObjectGetPropertyAtIndex(ctx, out.0 as _, 0, std::ptr::null_mut()) });
        Some(self.take(v))
    }

    fn clone_value<H: Host<Jsc>>(&mut self, host: &mut H, realm: Realm, v: Value) -> Option<Owned<Value>> {
        let f = self.helper(self.index(realm), Helper::Clone);
        let platform = self.platform_test();
        let out = self.call_value(host, f, UNDEFINED, &[v, platform]);
        let o = out.get();
        if Jsc::is_thrown(o) {
            return Some(out);
        }
        if !o.is_cell() {
            return None;
        }
        let ctx = self.realms[self.index(realm) as usize].ctx;
        // SAFETY: the array of one the helper just returned, which `out` holds
        let c = Value::from_ref(unsafe { ffi::JSObjectGetPropertyAtIndex(ctx, o.0 as _, 0, std::ptr::null_mut()) });
        let c = self.take(c);
        self.release(out);
        Some(c)
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

// whether its one argument is a platform object: a wrapper, of a class
// with private data, which the structured clone refuses
unsafe extern "C" fn is_platform(
    ctx: ffi::JSContextRef,
    _: ffi::JSObjectRef,
    _: ffi::JSObjectRef,
    argc: usize,
    argv: *const ffi::JSValueRef,
    _: *mut ffi::JSValueRef,
) -> ffi::JSValueRef {
    // SAFETY: the framework passes `argc` live arguments; the clone helper passes objects only
    let platform = argc == 1 && unsafe { ffi::JSValueGetType(ctx, *argv) == ffi::kJSTypeObject && !ffi::JSObjectGetPrivate((*argv).cast_mut()).is_null() };
    // SAFETY: a live context
    unsafe { ffi::JSValueMakeBoolean(ctx, platform) }
}

impl Jsc {
    /// The function the clone helper asks whether an object is a platform
    /// object, made once on the base context, so that it holds no realm.
    fn platform_test(&mut self) -> Value {
        if self.platform == Value(0) {
            // SAFETY: a function of a plain callback, on the isolate's live base context
            self.platform = Value::from_ref(unsafe { ffi::JSObjectMakeFunctionWithCallback(self.base, std::ptr::null_mut(), Some(is_platform)) }.cast_const());
            self.protect(self.platform);
        }
        self.platform
    }
}
