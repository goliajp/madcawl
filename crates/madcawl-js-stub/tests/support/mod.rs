//! What the engine tests share: a host that counts what it is told,
//! with one host object class, and helpers that act as script would.
#![allow(dead_code)]

use madcawl_js::{
    Attribute, AttributeSig, Bound, Builtins, ClassDef, ClassId, Control, Ctx, Effects, Engine, ErrorKind, Host, HostOps,
    HostRef, IdlType, Interceptors, Interrupt, Key, Owned,
};
use madcawl_js_stub::{Realm, Stub, Value};
use madcawl_str::DomString;

/// A host that counts what it is told, with one host object class.
#[derive(Default)]
pub struct Log {
    pub reported: u32,
    pub size: f64,
    pub callbacks: u32,
    pub pressure: u32,
    /// What `now` reads, in nanoseconds.
    pub clock: u64,
    /// The data word the last call of `data_word` got.
    pub data: u64,
    /// The current realm `size` last read or was written in.
    pub realm: Option<Realm>,
}

pub static OPS: HostOps<Stub, Log> = HostOps {
    traverse: |_, _, _| {},
    unlink: |_, _, _| {},
    can_skip: |_, _| true,
    wrapper_needs_preserve: |_, _| {},
    wrapper_died: |_, _| {},
};

// SAFETY: these functions read their context as Stub and Log
pub static INTERCEPTORS: Interceptors<Stub, Log> = Interceptors { index: Some(unsafe { Bound::index(item) }), length: None, named: None, iterable: false };

pub static CLASS: ClassDef<Stub, Log> = ClassDef {
    name: "List",
    id: ClassId(1),
    last: ClassId(1),
    parent: None,
    // SAFETY: as for INTERCEPTORS
    attributes: &[
        Attribute { name: "size", get: unsafe { Bound::host_fn(size) }, set: Some(unsafe { Bound::host_fn(set_size) }), sig: NUMBER },
        Attribute { name: "stop", get: unsafe { Bound::host_fn(size) }, set: Some(unsafe { Bound::host_fn(set_stop) }), sig: NUMBER },
    ],
    operations: &[],
    statics: &[],
    constants: &[],
    interceptors: Some(&INTERCEPTORS),
    ops: Some(&OPS),
    constructor: None,
    length: 0,
    error_prototype: false,
};

impl Host<Stub> for Log {
    fn class_of(&self, _: HostRef) -> &'static ClassDef<Stub, Log> {
        &CLASS
    }

    fn report_exception(&mut self, e: &mut Stub, exception: Owned<Value>) {
        self.reported += 1;
        e.release(exception);
    }

    fn load_imported_module(&mut self, _: &mut Stub, _: &madcawl_js::ModuleRequest<'_>, _: ()) {}

    fn interrupt_callback(&mut self, _: &mut Stub) {
        self.callbacks += 1;
    }

    fn memory_pressure(&mut self, _: &mut Stub) {
        self.pressure += 1;
    }

    fn now(&self) -> u64 {
        self.clock
    }
}

const NUMBER: AttributeSig = AttributeSig { ty: IdlType::Number, get: Effects::NONE, set: Effects::NONE };

/// A setter that terminates the isolate, as another thread would while
/// the host function runs, then tries to throw.
pub unsafe extern "C" fn set_stop(ctx: *mut Ctx, _this: u64, _argv: *const u64, _argc: usize) -> u64 {
    // SAFETY: called with a context for Log
    let (e, _) = unsafe { Ctx::parts::<Stub, Log>(ctx) };
    e.interrupt_handle().terminate();
    Stub::to_bits(e.throw_error(ErrorKind::Range, "too late").into_raw())
}

/// Records the data word of the function called.
pub unsafe extern "C" fn data_word(ctx: *mut Ctx, _this: u64, _argv: *const u64, _argc: usize) -> u64 {
    // SAFETY: called with a context for Log
    let data = unsafe { (*ctx).data };
    // SAFETY: as above
    let (_, h) = unsafe { Ctx::parts::<Stub, Log>(ctx) };
    h.data = data;
    Stub::to_bits(Stub::undefined())
}

/// Two items, 10 and 11.
pub unsafe extern "C" fn item(ctx: *mut Ctx, _this: u64, index: u32) -> u64 {
    // SAFETY: called with a context for Log
    let (e, _) = unsafe { Ctx::parts::<Stub, Log>(ctx) };
    assert!(e.in_script());
    match index {
        0 | 1 => Stub::to_bits(Stub::number(10.0 + index as f64)),
        _ => Stub::to_bits(Stub::absent()),
    }
}

pub unsafe extern "C" fn size(ctx: *mut Ctx, _this: u64, _argv: *const u64, argc: usize) -> u64 {
    assert_eq!(argc, 0, "a getter gets no arguments");
    // SAFETY: called with a context for Log
    let (e, h) = unsafe { Ctx::parts::<Stub, Log>(ctx) };
    h.realm = e.current_realm();
    Stub::to_bits(Stub::number(h.size))
}

pub unsafe extern "C" fn set_size(ctx: *mut Ctx, _this: u64, argv: *const u64, argc: usize) -> u64 {
    // SAFETY: called with a context for Log and its one argument
    let ((e, h), args) = unsafe { (Ctx::parts::<Stub, Log>(ctx), Stub::values(argv, argc)) };
    h.size = e.as_number(args[0]).unwrap();
    h.realm = e.current_realm();
    Stub::to_bits(Stub::undefined())
}

pub fn key(name: &str) -> DomString {
    DomString::from_wtf8(name.as_bytes())
}

pub fn num(e: &mut Stub, h: &mut Log, obj: Value, name: &str) -> f64 {
    let v = e.get(h, obj, Key::Str(&key(name)));
    let n = e.as_number(v.get()).unwrap_or(0.0);
    e.release(v);
    n
}

pub fn put(e: &mut Stub, h: &mut Log, obj: Value, name: &str, n: f64) {
    assert!(e.set(h, obj, Key::Str(&key(name)), Stub::number(n)));
}

pub fn bump(e: &mut Stub, h: &mut Log, obj: Value, name: &str) {
    let n = num(e, h, obj, name);
    put(e, h, obj, name, n + 1.0);
}

pub fn undefined() -> Owned<Value> {
    // SAFETY: undefined carries no count
    unsafe { Owned::from_raw(Stub::undefined()) }
}

/// A job that counts its runs in `name` on the object it closes over.
pub fn counter(e: &mut Stub, realm: madcawl_js_stub::Realm, log: Value, name: &'static str) -> Owned<Value> {
    let log = e.retain(log);
    let tag = e.string(key(name));
    e.new_closure::<Log>(
        realm,
        |e, h, caps, _, _| {
            let name = e.as_string(caps[1]).unwrap();
            bump(e, h, caps[0], name.as_ascii_str().unwrap());
            undefined()
        },
        vec![log, tag],
    )
}
