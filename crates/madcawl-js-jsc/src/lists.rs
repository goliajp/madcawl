//! The wrappers of a host class with an index getter (`NodeList`,
//! `HTMLCollection`): a framework class of their own, whose property
//! callbacks answer indices from the host, so that the class every other
//! wrapper derives from runs no callback on a property lookup.

use std::any::TypeId;

use madcawl_js::{Ctx, IndexGetter, Length};

use crate::ffi::{self, JSClassDefinition, JSContextRef, JSObjectRef, JSPropertyNameAccumulatorRef, JSStringRef, JSValueRef};
use crate::frame::{self, Frame};
use crate::isolate::Jsc;
use crate::objects::WrapRec;
use crate::value::{ABSENT, THROWN, Value};

type IndexThunk = unsafe fn(*mut Jsc, *mut (), IndexGetter, u64, u32) -> u64;
type LengthThunk = unsafe fn(*mut Jsc, *mut (), Length, u64) -> u32;

/// The index getter and length of a host class, and how to call them
/// with a context for its host type.
#[derive(Clone, Copy)]
pub(crate) struct ListCalls {
    index: IndexGetter,
    length: Length,
    index_thunk: IndexThunk,
    length_thunk: LengthThunk,
    host_type: TypeId,
}

impl ListCalls {
    pub(crate) fn of<H: 'static>(index: IndexGetter, length: Length) -> ListCalls {
        ListCalls { index, length, index_thunk: index_thunk::<H>, length_thunk: length_thunk::<H>, host_type: TypeId::of::<H>() }
    }
}

/// The property callbacks of a list wrapper class.
pub(crate) fn add_callbacks(def: &mut JSClassDefinition) {
    def.get_property = Some(get);
    def.has_property = Some(has);
    def.get_property_names = Some(names);
}

/// # Safety
/// `engine` and `host` are the frame's, `host` of type `H`.
unsafe fn index_thunk<H: 'static>(engine: *mut Jsc, host: *mut (), f: IndexGetter, this: u64, i: u32) -> u64 {
    // SAFETY: the caller's; the outer engine call is suspended in the
    // framework and does not use either until this returns
    unsafe {
        let mut ctx = Ctx::new(&mut *engine, &mut *host.cast::<H>(), 0);
        f(&mut ctx, this, i)
    }
}

/// # Safety
/// As for [`index_thunk`].
unsafe fn length_thunk<H: 'static>(engine: *mut Jsc, host: *mut (), f: Length, this: u64) -> u32 {
    // SAFETY: as for index_thunk
    unsafe {
        let mut ctx = Ctx::new(&mut *engine, &mut *host.cast::<H>(), 0);
        f(&mut ctx, this)
    }
}

/// The frame and the calls of wrapper `obj`, when its host may be asked:
/// inside a call that took the host, not being stopped, its realm alive.
fn target(obj: JSObjectRef) -> Option<(Frame, ListCalls)> {
    let f = frame::try_current()?;
    // SAFETY: the frame's engine, suspended in the framework
    let e = unsafe { &*f.engine };
    // SAFETY: the private data of a list class's object is its record
    let rec = unsafe { &*ffi::JSObjectGetPrivate(obj).cast::<WrapRec>() };
    if e.halted() || rec.orphan || e.realms[rec.realm as usize].detached {
        return None;
    }
    let calls = *e.classes.lists.get(&rec.id.0)?;
    assert_eq!(calls.host_type, f.host_type, "one host type per isolate");
    Some((f, calls))
}

fn length(obj: JSObjectRef) -> Option<u32> {
    let (f, c) = target(obj)?;
    // SAFETY: the frame's pointers, the host type checked
    Some(unsafe { (c.length_thunk)(f.engine, f.host, c.length, obj as u64) })
}

/// A canonical array index: digits without a leading zero, below 2^32 - 1.
fn index_of(name: JSStringRef) -> Option<u32> {
    // SAFETY: a live string the framework passed
    let units = unsafe {
        let len = ffi::JSStringGetLength(name);
        std::slice::from_raw_parts(ffi::JSStringGetCharactersPtr(name), len)
    };
    if units.is_empty() || units.len() > 10 || (units.len() > 1 && units[0] == u16::from(b'0')) {
        return None;
    }
    let mut n: u64 = 0;
    for &u in units {
        let d = u.checked_sub(u16::from(b'0')).filter(|&d| d < 10)?;
        n = n * 10 + u64::from(d);
    }
    u32::try_from(n).ok().filter(|&n| n != u32::MAX)
}

unsafe extern "C" fn get(_: JSContextRef, obj: JSObjectRef, name: JSStringRef, exception: *mut JSValueRef) -> JSValueRef {
    let Some(i) = index_of(name) else { return std::ptr::null() };
    let Some((f, c)) = target(obj) else { return std::ptr::null() };
    // SAFETY: the frame's pointers, the host type checked
    let r = Value(unsafe { (c.index_thunk)(f.engine, f.host, c.index, obj as u64, i) });
    // SAFETY: as above
    let e = unsafe { &mut *f.engine };
    if r == THROWN {
        let Some(x) = e.exception.take() else { return crate::objects::halted(exception) };
        // SAFETY: the framework's exception slot
        unsafe { *exception = x.raw() };
        e.unprotect(x);
        return std::ptr::null();
    }
    if r == ABSENT {
        return std::ptr::null();
    }
    // the result stays on this stack, which the collector scans, until
    // the framework has it
    e.unprotect(r);
    r.raw()
}

unsafe extern "C" fn has(_: JSContextRef, obj: JSObjectRef, name: JSStringRef) -> bool {
    index_of(name).is_some_and(|i| length(obj).is_some_and(|n| i < n))
}

unsafe extern "C" fn names(_: JSContextRef, obj: JSObjectRef, acc: JSPropertyNameAccumulatorRef) {
    let Some(n) = length(obj) else { return };
    let mut digits = [0u16; 10];
    for i in 0..n {
        let s = i.to_string();
        for (d, b) in digits.iter_mut().zip(s.bytes()) {
            *d = u16::from(b);
        }
        // SAFETY: `digits` holds the units for the call; the accumulator
        // keeps its own reference
        unsafe {
            let js = ffi::JSStringCreateWithCharacters(digits.as_ptr(), s.len());
            ffi::JSPropertyNameAccumulatorAddName(acc, js);
            ffi::JSStringRelease(js);
        }
    }
}
