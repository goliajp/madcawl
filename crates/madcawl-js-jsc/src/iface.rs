//! The callbacks of interface objects (`Node`, `Event`): calling one
//! throws, `new` runs its class's constructor when it has one, and
//! `instanceof` walks the prototype chain.

use crate::ffi::{self, JSContextRef, JSObjectRef, JSValueRef};
use crate::frame;
use crate::objects::{FnKind, FnRec, halted};
use crate::value::{THROWN, UNDEFINED, Value};

fn illegal(ctx: JSContextRef, exception: *mut JSValueRef) {
    let src = c"new TypeError('Illegal constructor')";
    // SAFETY: a live context inside its own callback
    unsafe {
        let s = ffi::JSStringCreateWithUTF8CString(src.as_ptr());
        *exception = ffi::JSEvaluateScript(ctx, s, std::ptr::null_mut(), std::ptr::null_mut(), 1, std::ptr::null_mut());
        ffi::JSStringRelease(s);
    }
}

pub(crate) unsafe extern "C" fn call(
    ctx: JSContextRef,
    _: JSObjectRef,
    _: JSObjectRef,
    _: usize,
    _: *const JSValueRef,
    exception: *mut JSValueRef,
) -> JSValueRef {
    illegal(ctx, exception);
    std::ptr::null()
}

/// `instanceof` an interface object: the framework asks the class of a
/// callback object and has no default, so this is OrdinaryHasInstance,
/// the value's prototype chain against the interface's `prototype`,
/// which throws when that is not an object.
pub(crate) unsafe extern "C" fn has_instance(ctx: JSContextRef, iface: JSObjectRef, v: JSValueRef, exception: *mut JSValueRef) -> bool {
    // SAFETY: a live context, interface object and value, inside the
    // framework's own callback
    unsafe {
        if ffi::JSValueGetType(ctx, v) != ffi::kJSTypeObject {
            return false;
        }
        let name = ffi::JSStringCreateWithUTF8CString(c"prototype".as_ptr());
        let proto = ffi::JSObjectGetProperty(ctx, iface, name, exception);
        ffi::JSStringRelease(name);
        if !(*exception).is_null() {
            return false;
        }
        if ffi::JSValueGetType(ctx, proto) != ffi::kJSTypeObject {
            let src = c"new TypeError('the prototype of the right-hand side of instanceof is not an object')";
            let s = ffi::JSStringCreateWithUTF8CString(src.as_ptr());
            *exception = ffi::JSEvaluateScript(ctx, s, std::ptr::null_mut(), std::ptr::null_mut(), 1, std::ptr::null_mut());
            ffi::JSStringRelease(s);
            return false;
        }
        let mut o = v;
        loop {
            o = ffi::JSObjectGetPrototype(ctx, o as JSObjectRef);
            if ffi::JSValueGetType(ctx, o) != ffi::kJSTypeObject {
                return false;
            }
            if ffi::JSValueIsStrictEqual(ctx, o, proto) {
                return true;
            }
        }
    }
}

/// `new` on an interface object: its class's constructor, a host
/// function called with undefined as `this`, whose record is the
/// interface object's private data.
pub(crate) unsafe extern "C" fn construct(
    ctx: JSContextRef,
    iface: JSObjectRef,
    argc: usize,
    argv: *const JSValueRef,
    exception: *mut JSValueRef,
) -> JSObjectRef {
    // SAFETY: an interface object's private data is its record, or null
    let Some(rec) = (unsafe { ffi::JSObjectGetPrivate(iface).cast::<FnRec>().as_ref() }) else {
        illegal(ctx, exception);
        return std::ptr::null_mut();
    };
    let f = frame::current();
    // SAFETY: the frame's engine, suspended in the framework
    let e = unsafe { &mut *f.engine };
    let FnKind::Host { f: hf, data, thunk } = rec.kind else { unreachable!("a constructor is a host function") };
    if e.halted() {
        return halted(exception) as JSObjectRef;
    }
    assert_eq!(rec.host_type, f.host_type, "one host type per isolate");
    // SAFETY: the frame's pointers; `JSValueRef` is the value word
    let r = Value(unsafe { thunk(f.engine, f.host, hf, data, UNDEFINED.0, argv.cast(), argc) });
    // SAFETY: as above
    let e = unsafe { &mut *f.engine };
    if r == THROWN {
        let Some(x) = e.exception.take() else { return halted(exception) as JSObjectRef };
        // SAFETY: the framework's exception slot
        unsafe { *exception = x.raw() };
        e.unprotect(x);
        return std::ptr::null_mut();
    }
    e.unprotect(r);
    r.0 as JSObjectRef
}

