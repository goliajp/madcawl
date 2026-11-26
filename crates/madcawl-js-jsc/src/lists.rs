//! The wrappers of a host class with an index getter (`NodeList`,
//! `HTMLCollection`): a framework class of their own, whose property
//! callbacks answer indices, and names when the class has a named
//! getter, from the host, so that the class every other wrapper derives
//! from runs no callback on a property lookup.
//!
//! The framework gives a property a callback answers the attributes
//! read-only and not enumerable, and has no callback for defining one:
//! an index reads as not enumerable, and `Object.defineProperty` puts an
//! own property beside the indices (hidden by them while in range). A
//! write to an index, or to a name the named getter answers, is refused
//! silently, in strict code too, and a delete of an index answers true
//! and leaves it. Named properties are not listed by
//! `Object.getOwnPropertyNames`.

use std::any::TypeId;
use std::cell::Cell;

use madcawl_js::{Ctx, IndexGetter, Length, NamedGetter};

use crate::ffi::{self, JSClassDefinition, JSContextRef, JSObjectRef, JSPropertyNameAccumulatorRef, JSStringRef, JSValueRef};
use crate::frame::{self, Frame};
use crate::isolate::Jsc;
use crate::objects::WrapRec;
use crate::value::{ABSENT, THROWN, Value};

type IndexThunk = unsafe fn(*mut Jsc, *mut (), IndexGetter, u64, u32) -> u64;
type LengthThunk = unsafe fn(*mut Jsc, *mut (), Length, u64) -> u32;
type NamedThunk = unsafe fn(*mut Jsc, *mut (), NamedGetter, u64, u64) -> u64;

/// The index getter and length of a host class, and how to call them
/// with a context for its host type.
#[derive(Clone, Copy)]
pub(crate) struct ListCalls {
    index: IndexGetter,
    length: Length,
    index_thunk: IndexThunk,
    length_thunk: LengthThunk,
    named: Option<(NamedGetter, NamedThunk)>,
    host_type: TypeId,
}

impl ListCalls {
    pub(crate) fn of<H: 'static>(index: IndexGetter, length: Length, named: Option<NamedGetter>) -> ListCalls {
        let named = named.map(|f| (f, named_thunk::<H> as NamedThunk));
        ListCalls { index, length, index_thunk: index_thunk::<H>, length_thunk: length_thunk::<H>, named, host_type: TypeId::of::<H>() }
    }
}

thread_local! {
    /// The wrapper a named lookup is asking whether it or a prototype
    /// has the name, or null: the get callback answers no name of that
    /// wrapper meanwhile, and those of every other.
    static OWN_LOOKUP: Cell<JSObjectRef> = const { Cell::new(std::ptr::null_mut()) };
}

/// The property callbacks of a list wrapper class. No `has` callback:
/// the framework asks `get` for `in` too, and a `has` would make every
/// read ask the host twice.
pub(crate) fn add_callbacks(def: &mut JSClassDefinition) {
    def.get_property = Some(get);
    def.set_property = Some(set);
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

/// # Safety
/// As for [`index_thunk`].
unsafe fn named_thunk<H: 'static>(engine: *mut Jsc, host: *mut (), f: NamedGetter, this: u64, name: u64) -> u64 {
    // SAFETY: as for index_thunk
    unsafe {
        let mut ctx = Ctx::new(&mut *engine, &mut *host.cast::<H>(), 0);
        f(&mut ctx, this, name)
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
    let len = unsafe { ffi::JSStringGetLength(name) };
    if len == 0 {
        return None;
    }
    // SAFETY: as above; a string that is not empty has its units
    let units = unsafe { std::slice::from_raw_parts(ffi::JSStringGetCharactersPtr(name), len) };
    if units.len() > 10 || (units.len() > 1 && units[0] == u16::from(b'0')) {
        return None;
    }
    let mut n: u64 = 0;
    for &u in units {
        let d = u.checked_sub(u16::from(b'0')).filter(|&d| d < 10)?;
        n = n * 10 + u64::from(d);
    }
    u32::try_from(n).ok().filter(|&n| n != u32::MAX)
}

unsafe extern "C" fn get(ctx: JSContextRef, obj: JSObjectRef, name: JSStringRef, exception: *mut JSValueRef) -> JSValueRef {
    let Some(i) = index_of(name) else {
        // SAFETY: the framework's arguments
        return unsafe { get_named(ctx, obj, name, exception) };
    };
    let Some((f, c)) = target(obj) else { return std::ptr::null() };
    // SAFETY: the frame's pointers, the host type checked
    let r = Value(unsafe { (c.index_thunk)(f.engine, f.host, c.index, obj as u64, i) });
    // SAFETY: the frame's engine and the framework's exception slot
    unsafe { answer(&mut *f.engine, r, exception) }
}

/// A name that is no index: the host's named getter, when the class has
/// one and neither `obj` nor a prototype has the name (Web IDL's named
/// property visibility; lookup goes on to them otherwise).
///
/// # Safety
/// The framework's arguments to a get callback.
unsafe fn get_named(ctx: JSContextRef, obj: JSObjectRef, name: JSStringRef, exception: *mut JSValueRef) -> JSValueRef {
    if OWN_LOOKUP.get() == obj || target(obj).is_none_or(|(_, c)| c.named.is_none()) {
        return std::ptr::null();
    }
    // SAFETY: a live string; the result stays on this stack, which the
    // collector scans, through the calls
    let s = unsafe { ffi::JSValueMakeString(ctx, name) };
    let outer = OWN_LOOKUP.replace(obj);
    let mut thrown: JSValueRef = std::ptr::null();
    // SAFETY: live arguments; the lookup may run script (a proxy on the
    // prototype chain), which leaves the frame as it was and may throw
    let found = unsafe { ffi::JSObjectHasPropertyForKey(ctx, obj, s, &mut thrown) };
    OWN_LOOKUP.set(outer);
    if !thrown.is_null() {
        // SAFETY: the framework's exception slot
        unsafe { *exception = thrown };
        return std::ptr::null();
    }
    // script may have stopped the engine meanwhile
    let Some((f, c)) = target(obj).filter(|_| !found) else { return std::ptr::null() };
    let (named, thunk) = c.named.expect("checked above");
    // SAFETY: the frame's pointers, the host type checked
    let r = Value(unsafe { thunk(f.engine, f.host, named, obj as u64, s as u64) });
    // SAFETY: as in get
    unsafe { answer(&mut *f.engine, r, exception) }
}

/// The framework's answer for a getter's result `r`, counted.
///
/// # Safety
/// `exception` is the framework's exception slot.
unsafe fn answer(e: &mut Jsc, r: Value, exception: *mut JSValueRef) -> JSValueRef {
    if r == THROWN {
        let Some(x) = e.exception.take() else { return crate::objects::halted(exception) };
        // SAFETY: the caller's
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

/// A write to an index, or to a name the named getter answers, is
/// refused: it neither reaches the host nor makes an own property.
unsafe extern "C" fn set(ctx: JSContextRef, obj: JSObjectRef, name: JSStringRef, _: JSValueRef, exception: *mut JSValueRef) -> bool {
    if index_of(name).is_some() {
        return true;
    }
    let Some((f, c)) = target(obj) else { return false };
    let Some((named, thunk)) = c.named else { return false };
    // SAFETY: as in get_named
    let s = unsafe { ffi::JSValueMakeString(ctx, name) };
    // SAFETY: the frame's pointers, the host type checked
    let r = Value(unsafe { thunk(f.engine, f.host, named, obj as u64, s as u64) });
    // SAFETY: the frame's engine and the framework's exception slot
    let found = unsafe { answer(&mut *f.engine, r, exception) };
    // SAFETY: the framework's exception slot
    !found.is_null() || unsafe { !(*exception).is_null() }
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
