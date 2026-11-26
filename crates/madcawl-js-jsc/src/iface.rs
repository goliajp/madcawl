//! The callbacks of the interface objects of classes without a
//! constructor (`Node`): calling or constructing one throws, and
//! `instanceof` walks the prototype chain.

use crate::ffi::{self, JSContextRef, JSObjectRef, JSValueRef};
use crate::frame;
use crate::objects::halted;

/// Put a TypeError saying `message` in `exception`, made by the realm's
/// captured constructor, so a page that replaced `TypeError` does not
/// get its own object.
fn throw_type(exception: *mut JSValueRef, message: &str) {
    // SAFETY: the frame's engine, suspended in the framework
    let e = unsafe { &mut *frame::current().engine };
    if e.halted() {
        halted(exception);
        return;
    }
    let _sentinel = e.throw_new(madcawl_js::ErrorKind::Type, message).into_raw();
    match e.exception.take() {
        Some(x) => {
            // SAFETY: the framework's exception slot; the value stays on
            // the stack the collector scans until the framework has it
            unsafe { *exception = x.raw() };
            e.unprotect(x);
        }
        None => {
            halted(exception);
        }
    }
}

fn illegal(_: JSContextRef, exception: *mut JSValueRef) {
    throw_type(exception, "Illegal constructor");
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
            throw_type(exception, "the prototype of the right-hand side of instanceof is not an object");
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

/// `new` on an interface object of a class without a constructor
/// (one with a constructor is a script function, `protos.rs`).
pub(crate) unsafe extern "C" fn construct(
    ctx: JSContextRef,
    _: JSObjectRef,
    _: usize,
    _: *const JSValueRef,
    exception: *mut JSValueRef,
) -> JSObjectRef {
    illegal(ctx, exception);
    std::ptr::null_mut()
}
