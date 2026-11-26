//! The framework classes the adapter makes objects of, their records,
//! and their callbacks.
//!
//! - A *function* object calls a host function ([`FnKind::Host`]) or
//!   runs a host microtask ([`FnKind::Micro`]); its prototype is the
//!   realm's `Function.prototype`, so `call`, `apply` and `bind` work.
//! - An *interface* object (`Node`, `Element`) throws when called or
//!   constructed, as an interface without a constructor does.
//! - A *wrapper* stands for a host object. Its record outlives it until
//!   the host is told it died, so a wrapper word the host keeps is
//!   always safe to read.

use std::any::TypeId;
use std::ffi::c_void;

use madcawl_js::{ClassId, HostFn, HostRef};

use crate::ffi::{self, JSClassDefinition, JSClassRef, JSContextRef, JSObjectRef, JSValueRef};
use crate::frame;
use crate::isolate::{Jsc, Shared};
use crate::value::{THROWN, UNDEFINED, Value};

/// Calls host function `f` with a context made for the host type it
/// was instantiated for.
pub(crate) type Thunk = unsafe fn(*mut Jsc, *mut (), HostFn, u64, u64, *const u64, usize) -> u64;
/// Hands an exception nothing caught to the host.
pub(crate) type Report = unsafe fn(*mut Jsc, *mut (), Value);

pub(crate) enum FnKind {
    Host { f: HostFn, data: u64, thunk: Thunk },
    /// `job(arg)`, both counted until it ran.
    Micro { job: Value, arg: Value, realm: u32 },
    Ran,
}

pub(crate) struct FnRec {
    pub kind: FnKind,
    pub host_type: TypeId,
}

pub(crate) struct WrapRec {
    pub obj: HostRef,
    /// The class table, as `&'static ClassDef<Jsc, H>` for `host_type`.
    pub def: *const (),
    pub id: ClassId,
    pub host_type: TypeId,
    pub js: Value,
    /// Finalized: its notice is queued.
    pub dead: bool,
    /// Refused by `wrapper_upgrade`: the host let go of it already.
    pub orphan: bool,
    pub shared: *const Shared,
}

pub(crate) struct Classes {
    pub func: JSClassRef,
    pub iface: JSClassRef,
    pub wrapper: JSClassRef,
}

impl Classes {
    pub(crate) fn new() -> Classes {
        let make = |name: &'static std::ffi::CStr, f: fn(&mut JSClassDefinition)| {
            let mut def = JSClassDefinition::EMPTY;
            def.class_name = name.as_ptr();
            def.attributes = ffi::kJSClassAttributeNoAutomaticPrototype;
            f(&mut def);
            // SAFETY: a complete definition with static strings
            unsafe { ffi::JSClassCreate(&def) }
        };
        Classes {
            func: make(c"Function", |d| {
                d.call_as_function = Some(call_fn);
                d.finalize = Some(finalize_fn);
            }),
            iface: make(c"Function", |d| {
                d.call_as_function = Some(call_iface);
                d.call_as_constructor = Some(construct_iface);
            }),
            wrapper: make(c"Object", |d| d.finalize = Some(finalize_wrapper)),
        }
    }

    pub(crate) fn release(&mut self) {
        for c in [self.func, self.iface, self.wrapper] {
            // SAFETY: made in new, no object of them is left
            unsafe { ffi::JSClassRelease(c) };
        }
    }
}

/// # Safety
/// `engine` and `host` are the frame's, `host` of type `H`.
pub(crate) unsafe fn thunk<H: 'static>(
    engine: *mut Jsc,
    host: *mut (),
    f: HostFn,
    data: u64,
    this: u64,
    argv: *const u64,
    argc: usize,
) -> u64 {
    // SAFETY: the caller's; the outer engine call is suspended in the
    // framework and does not use either until this returns
    unsafe {
        let (e, h) = (&mut *engine, &mut *host.cast::<H>());
        let mut ctx = madcawl_js::Ctx::new(e, h, data);
        f(&mut ctx, this, argv, argc)
    }
}

/// # Safety
/// As for [`thunk`]; `exception` carries one count.
pub(crate) unsafe fn report<H: madcawl_js::Host<Jsc>>(engine: *mut Jsc, host: *mut (), exception: Value) {
    // SAFETY: the caller's
    let (e, h) = unsafe { (&mut *engine, &mut *host.cast::<H>()) };
    h.report_exception(e, Jsc::owned(exception));
}

unsafe extern "C" fn call_fn(
    ctx: JSContextRef,
    function: JSObjectRef,
    this: JSObjectRef,
    argc: usize,
    argv: *const JSValueRef,
    exception: *mut JSValueRef,
) -> JSValueRef {
    let f = frame::current();
    // SAFETY: the private data of a function object is its record
    let rec = unsafe { &mut *ffi::JSObjectGetPrivate(function).cast::<FnRec>() };
    // SAFETY: the frame's engine, suspended in the framework
    let e = unsafe { &mut *f.engine };
    if e.halted() {
        return UNDEFINED.raw();
    }
    match rec.kind {
        FnKind::Host { f: hf, data, thunk } => {
            assert_eq!(rec.host_type, f.host_type, "one host type per isolate");
            let this = if this.is_null() { UNDEFINED.0 } else { this as u64 };
            // SAFETY: the frame's pointers; `JSValueRef` is the value word
            let r = Value(unsafe { thunk(f.engine, f.host, hf, data, this, argv.cast(), argc) });
            // SAFETY: as above
            let e = unsafe { &mut *f.engine };
            if r == THROWN {
                match e.exception.take() {
                    Some(x) => {
                        // SAFETY: the framework's exception slot
                        unsafe { *exception = x.raw() };
                        e.unprotect(x);
                    }
                    None => return UNDEFINED.raw(),
                }
                return std::ptr::null();
            }
            // the result stays on this stack, which the collector scans,
            // until the framework has it
            e.unprotect(r);
            r.raw()
        }
        FnKind::Micro { .. } => {
            crate::micro::run_job(e, ctx, rec, f.host, f.report);
            UNDEFINED.raw()
        }
        FnKind::Ran => UNDEFINED.raw(),
    }
}

unsafe extern "C" fn finalize_fn(obj: JSObjectRef) {
    // SAFETY: the record made with the object, freed only here
    drop(unsafe { Box::from_raw(ffi::JSObjectGetPrivate(obj).cast::<FnRec>()) });
}

unsafe extern "C" fn finalize_wrapper(obj: JSObjectRef) {
    // SAFETY: the record made with the wrapper; it lives until the host
    // is told
    let rec = unsafe { &mut *ffi::JSObjectGetPrivate(obj).cast::<WrapRec>() };
    rec.dead = true;
    // SAFETY: the isolate's shared state outlives its heap
    unsafe { &*rec.shared }.notices.borrow_mut().push(rec);
}

fn illegal(ctx: JSContextRef, exception: *mut JSValueRef) {
    let src = c"new TypeError('Illegal constructor')";
    // SAFETY: a live context inside its own callback
    unsafe {
        let s = ffi::JSStringCreateWithUTF8CString(src.as_ptr());
        *exception = ffi::JSEvaluateScript(ctx, s, std::ptr::null_mut(), std::ptr::null_mut(), 1, std::ptr::null_mut());
        ffi::JSStringRelease(s);
    }
}

unsafe extern "C" fn call_iface(
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

unsafe extern "C" fn construct_iface(
    ctx: JSContextRef,
    _: JSObjectRef,
    _: usize,
    _: *const JSValueRef,
    exception: *mut JSValueRef,
) -> JSObjectRef {
    illegal(ctx, exception);
    std::ptr::null_mut()
}

/// A function object for `rec` in the context `ctx`, whose prototype
/// is `function_proto`; with a count.
pub(crate) fn make_function(e: &Jsc, ctx: JSContextRef, rec: FnRec, function_proto: Value) -> Value {
    let data = Box::into_raw(Box::new(rec)).cast::<c_void>();
    // SAFETY: the record is freed by the class's finalizer
    let obj = unsafe { ffi::JSObjectMake(ctx, e.classes.func, data) };
    let v = Value(obj as u64);
    e.protect(v);
    // SAFETY: a live object and prototype of the same isolate
    unsafe { ffi::JSObjectSetPrototype(ctx, obj, function_proto.raw()) };
    v
}
