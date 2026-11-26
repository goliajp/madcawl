//! Calls into the framework that can run script: each enters with the
//! host as the frame, counts in the depth, and turns the framework's
//! exception out-parameter into the registered exception and the
//! thrown sentinel.

use madcawl_js::{Host, Key, Owned, ScriptOrigin};
use madcawl_str::DomString;

use crate::ffi::{self, JSObjectRef, JSValueRef};
use crate::frame;
use crate::isolate::{Jsc, Realm};
use crate::protos::Helper;
use crate::value::{FALSE, NULL, THROWN, TRUE, UNDEFINED, Value};

impl Jsc {
    /// Run `f` inside the framework; `None` when it threw, with the
    /// exception registered (none when script was being stopped).
    pub(crate) fn run<H: Host<Jsc>, R>(&mut self, host: &mut H, f: impl FnOnce(&mut Jsc, *mut JSValueRef) -> R) -> Option<R> {
        self.poll(host);
        if self.halted() {
            return None;
        }
        let me: *mut Jsc = self;
        let _frame = frame::enter(me, host);
        let mut exc: JSValueRef = std::ptr::null();
        self.depth += 1;
        let r = f(self, &mut exc);
        self.depth -= 1;
        if exc.is_null() {
            return Some(r);
        }
        self.after_stop();
        if !self.halted() {
            self.register(Value::from_ref(exc));
        }
        None
    }

    /// As [`Jsc::run`] for a call that returns a value: it, counted, or
    /// thrown.
    pub(crate) fn run_value<H: Host<Jsc>>(&mut self, host: &mut H, f: impl FnOnce(&mut Jsc, *mut JSValueRef) -> JSValueRef) -> Owned<Value> {
        match self.run(host, f) {
            Some(r) => self.take(Value::from_ref(r)),
            None => Jsc::owned(THROWN),
        }
    }

    pub(crate) fn register(&mut self, x: Value) {
        self.protect(x);
        if let Some(old) = self.exception.replace(x) {
            self.unprotect(old);
        }
    }

    /// `v` as an object, boxing a primitive; the framework throws for
    /// undefined and null.
    fn object(e: &Jsc, v: Value, exc: *mut JSValueRef) -> JSObjectRef {
        // SAFETY: a live value
        unsafe { ffi::JSValueToObject(e.ctx(), v.raw(), exc) }
    }

    pub(crate) fn get_prop<H: Host<Jsc>>(&mut self, host: &mut H, obj: Value, key: Key<'_>) -> Owned<Value> {
        let name = self.key_str(key);
        self.run_value(host, |e, exc| {
            let o = Jsc::object(e, obj, exc);
            if o.is_null() {
                return std::ptr::null();
            }
            // SAFETY: a live object; the name lives for the call
            unsafe {
                match &name {
                    Ok(n) => ffi::JSObjectGetProperty(e.ctx(), o, n.s, exc),
                    Err(i) => ffi::JSObjectGetPropertyAtIndex(e.ctx(), o, *i, exc),
                }
            }
        })
    }

    pub(crate) fn set_prop<H: Host<Jsc>>(&mut self, host: &mut H, obj: Value, key: Key<'_>, v: Value) -> bool {
        let name = self.key_str(key);
        self.run(host, |e, exc| {
            let o = Jsc::object(e, obj, exc);
            if o.is_null() {
                return;
            }
            // SAFETY: as in get_prop
            unsafe {
                match &name {
                    Ok(n) => ffi::JSObjectSetProperty(e.ctx(), o, n.s, v.raw(), 0, exc),
                    Err(i) => ffi::JSObjectSetPropertyAtIndex(e.ctx(), o, *i, v.raw(), exc),
                }
            }
        })
        .is_some()
    }

    pub(crate) fn delete_prop<H: Host<Jsc>>(&mut self, host: &mut H, obj: Value, key: Key<'_>) -> bool {
        let name = match self.key_str(key) {
            Ok(n) => n,
            Err(i) => self.js_str_utf8(&i.to_string()),
        };
        self.run(host, |e, exc| {
            let o = Jsc::object(e, obj, exc);
            if !o.is_null() {
                // SAFETY: as in get_prop
                unsafe { ffi::JSObjectDeleteProperty(e.ctx(), o, name.s, exc) };
            }
        })
        .is_some()
    }

    pub(crate) fn value_kind(&self, v: Value) -> madcawl_js::ValueKind {
        use madcawl_js::ValueKind;
        match v {
            UNDEFINED => return ValueKind::Undefined,
            NULL => return ValueKind::Null,
            TRUE | FALSE => return ValueKind::Boolean,
            _ if v.as_number().is_some() => return ValueKind::Number,
            _ => {}
        }
        // SAFETY: a live cell
        match unsafe { ffi::JSValueGetType(self.base, v.raw()) } {
            ffi::kJSTypeString => ValueKind::String,
            ffi::kJSTypeSymbol => ValueKind::Symbol,
            ffi::kJSTypeObject if self.callable(v) => ValueKind::Function,
            _ => ValueKind::Object,
        }
    }

    pub(crate) fn callable(&self, v: Value) -> bool {
        // SAFETY: type checks on a live value
        v.is_cell()
            && unsafe {
                ffi::JSValueGetType(self.base, v.raw()) == ffi::kJSTypeObject && ffi::JSObjectIsFunction(self.base, v.0 as _)
            }
    }

    pub(crate) fn call_value<H: Host<Jsc>>(&mut self, host: &mut H, f: Value, this: Value, args: &[Value]) -> Owned<Value> {
        if !self.callable(f) {
            return self.throw_new(madcawl_js::ErrorKind::Type, "not a function");
        }
        // SAFETY: a type check on a live value
        if this.is_cell() && unsafe { ffi::JSValueGetType(self.base, this.raw()) } == ffi::kJSTypeObject {
            let this = this.0 as JSObjectRef;
            return self.run_value(host, |e, exc| {
                // SAFETY: a live function and arguments; `Value` is the word
                unsafe { ffi::JSObjectCallAsFunction(e.ctx(), f.0 as _, this, args.len(), args.as_ptr().cast(), exc) }
            });
        }
        // the framework takes only an object as `this`, and passes the
        // global for none: anything else goes through
        // `Function.prototype.call`, which passes it as it is
        let call = self.helper(self.current, Helper::Call);
        let mut all = Vec::with_capacity(args.len() + 1);
        all.push(this);
        all.extend_from_slice(args);
        self.run_value(host, |e, exc| {
            // SAFETY: as above, with `f` as the `this` of `call`
            unsafe { ffi::JSObjectCallAsFunction(e.ctx(), call.0 as _, f.0 as _, all.len(), all.as_ptr().cast(), exc) }
        })
    }

    pub(crate) fn eval_script<H: Host<Jsc>>(&mut self, host: &mut H, realm: Realm, source: &DomString, origin: &ScriptOrigin<'_>) -> Owned<Value> {
        let ctx = self.realm_ctx(realm);
        let src = self.js_str(source);
        let url = self.js_str(origin.url);
        let line = i32::try_from(origin.line).unwrap_or(i32::MAX);
        self.run_value(host, |_, exc| {
            // SAFETY: live strings and context
            unsafe { ffi::JSEvaluateScript(ctx, src.s, std::ptr::null_mut(), url.s, line, exc) }
        })
    }

    pub(crate) fn handler<H: Host<Jsc>>(&mut self, host: &mut H, realm: Realm, params: &[&str], body: &DomString, scopes: &[Value]) -> Owned<Value> {
        let ctx = self.realm_ctx(realm);
        let names: Vec<Value> = params.iter().map(|p| self.string_of(&DomString::from_wtf8(p.as_bytes()))).collect();
        let params = self.array(ctx, &names);
        let scopes = self.array(ctx, scopes);
        let body = self.string_of(body);
        let f = self.helper(realm.0, Helper::Handler);
        let args = [params, body, scopes];
        let r = self.run_value(host, |_, exc| {
            // SAFETY: a helper of a live realm
            unsafe { ffi::JSObjectCallAsFunction(ctx, f.0 as _, std::ptr::null_mut(), 3, args.as_ptr().cast(), exc) }
        });
        for v in names.into_iter().chain(args) {
            self.unprotect(v);
        }
        r
    }

    pub(crate) fn convert_string<H: Host<Jsc>>(&mut self, host: &mut H, v: Value) -> Option<DomString> {
        if let Some(s) = self.string_value(v) {
            return Some(s);
        }
        let s = self.run(host, |e, exc| {
            // SAFETY: a live value; may run script
            unsafe { ffi::JSValueToStringCopy(e.ctx(), v.raw(), exc) }
        })?;
        if s.is_null() {
            return None;
        }
        let out = Jsc::dom_string(s);
        // SAFETY: the copy made above
        unsafe { ffi::JSStringRelease(s) };
        Some(out)
    }

    pub(crate) fn convert_number<H: Host<Jsc>>(&mut self, host: &mut H, v: Value) -> Option<f64> {
        if let Some(n) = v.as_number() {
            return Some(n);
        }
        self.run(host, |e, exc| {
            // SAFETY: a live value; may run script
            unsafe { ffi::JSValueToNumber(e.ctx(), v.raw(), exc) }
        })
    }

    /// Throw a new error of `kind` made in the current realm. Making it
    /// runs script, so it can itself be stopped: then nothing is
    /// registered, as for any call script was being ended in.
    pub(crate) fn throw_new(&mut self, kind: madcawl_js::ErrorKind, message: &str) -> Owned<Value> {
        use madcawl_js::ErrorKind::*;
        let k = match kind {
            Type => 0.0,
            Range => 1.0,
            Syntax => 2.0,
            Reference => 3.0,
        };
        let msg = self.string_of(&DomString::from_wtf8(message.as_bytes()));
        let r = self.call_helper(self.current, Helper::Error, &[Value::number(k), msg]);
        self.unprotect(msg);
        match r {
            Ok(e) if !self.halted() => self.register(e),
            Ok(_) => {}
            Err(x) => {
                self.after_stop();
                if !self.halted() {
                    self.register(x);
                }
            }
        }
        Jsc::owned(THROWN)
    }
}
