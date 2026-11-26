//! ToString, ToNumber and ToBoolean, with ToPrimitive calling an object's own
//! `toString` and `valueOf` as script defines them. With neither, an
//! object converts as `Object.prototype` would make it, which the stub
//! does not have.

use madcawl_js::{Engine, ErrorKind, Host, Key, Owned, ValueKind};
use madcawl_str::DomString;

use crate::isolate::Stub;
use crate::value::{THROWN, TRUE, Value};

impl Stub {
    pub(crate) fn truthy(&self, v: Value) -> bool {
        match self.kind(v) {
            ValueKind::Undefined | ValueKind::Null => false,
            ValueKind::Boolean => v == TRUE,
            ValueKind::Number => v.as_number().is_some_and(|n| n != 0.0 && !n.is_nan()),
            ValueKind::String => self.string_len(v).is_some_and(|n| n != 0),
            _ => true,
        }
    }

    pub(crate) fn string_of<H: Host<Stub>>(&mut self, host: &mut H, v: Value) -> Option<DomString> {
        if let Some(s) = self.as_string(v) {
            return Some(s);
        }
        let text = match self.kind(v) {
            ValueKind::Undefined => "undefined".to_owned(),
            ValueKind::Null => "null".to_owned(),
            ValueKind::Boolean => (v == TRUE).to_string(),
            ValueKind::Number => number_text(v.as_number().expect("a number")),
            ValueKind::Symbol => return self.fail(ErrorKind::Type, "cannot convert a Symbol to a string"),
            ValueKind::String => unreachable!("as_string took it"),
            ValueKind::Object | ValueKind::Function => {
                let p = self.primitive_of(host, v, ["toString", "valueOf"])?;
                let s = self.string_of(host, p.get());
                self.release(p);
                return s;
            }
        };
        Some(DomString::from_wtf8(text.as_bytes()))
    }

    pub(crate) fn number_of<H: Host<Stub>>(&mut self, host: &mut H, v: Value) -> Option<f64> {
        Some(match self.kind(v) {
            ValueKind::Number => v.as_number().expect("a number"),
            ValueKind::Boolean => (v == TRUE) as u8 as f64,
            ValueKind::Null => 0.0,
            ValueKind::Undefined => f64::NAN,
            ValueKind::String => {
                let mut b = Vec::new();
                self.as_string(v).expect("a string").write_wtf8(&mut b);
                let t = String::from_utf8_lossy(&b);
                let t = t.trim();
                if t.is_empty() { 0.0 } else { t.parse().unwrap_or(f64::NAN) }
            }
            ValueKind::Symbol => return self.fail(ErrorKind::Type, "cannot convert a Symbol to a number"),
            ValueKind::Object | ValueKind::Function => {
                let p = self.primitive_of(host, v, ["valueOf", "toString"])?;
                let n = self.number_of(host, p.get());
                self.release(p);
                return n;
            }
        })
    }

    /// OrdinaryToPrimitive: the first of `order` that is callable and
    /// returns no object; `None` when one threw or none did. With no
    /// method at all, the text `Object.prototype` gives.
    fn primitive_of<H: Host<Stub>>(&mut self, host: &mut H, v: Value, order: [&str; 2]) -> Option<Owned<Value>> {
        let mut any = false;
        for name in order {
            let f = self.get_prop(host, v, Key::Str(&DomString::from_wtf8(name.as_bytes())));
            if f.get() == THROWN {
                return None;
            }
            if !self.is_callable(f.get()) {
                self.release(f);
                continue;
            }
            any = true;
            let r = self.call_value(host, f.get(), v, &[]);
            self.release(f);
            if r.get() == THROWN {
                return None;
            }
            if !matches!(self.kind(r.get()), ValueKind::Object | ValueKind::Function) {
                return Some(r);
            }
            self.release(r);
        }
        if any {
            return self.fail(ErrorKind::Type, "cannot convert an object to a primitive value");
        }
        let text: &[u8] = if self.is_callable(v) { b"function () { [native code] }" } else { b"[object Object]" };
        Some(self.string(DomString::from_wtf8(text)))
    }

    fn fail<T>(&mut self, kind: ErrorKind, message: &str) -> Option<T> {
        let _sentinel = self.throw_error(kind, message).into_raw();
        None
    }
}

fn number_text(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_owned()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity" } else { "-Infinity" }.to_owned()
    } else if n == n.trunc() && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}
