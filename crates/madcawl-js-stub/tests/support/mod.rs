//! What the engine tests share: a host that counts what it is told,
//! with one host object class, and helpers that act as script would.
#![allow(dead_code)]

use madcawl_js::{
    Attribute, Bound, ClassDef, ClassId, Ctx, Engine, Host, HostOps, HostRef, Interceptors, Key, Owned,
};
use madcawl_js_stub::{Stub, Value};
use madcawl_str::DomString;

/// A host that counts what it is told, with one host object class.
#[derive(Default)]
pub struct Log {
    pub reported: u32,
    pub size: f64,
}

pub static OPS: HostOps<Stub, Log> = HostOps {
    traverse: |_, _, _| {},
    unlink: |_, _, _| {},
    can_skip: |_, _| true,
    wrapper_needs_preserve: |_, _| {},
    wrapper_died: |_, _| {},
};

// SAFETY: these functions read their context as Stub and Log
pub static INTERCEPTORS: Interceptors<Stub, Log> = Interceptors { index: Some(unsafe { Bound::index(item) }), length: None };

pub static CLASS: ClassDef<Stub, Log> = ClassDef {
    name: "List",
    id: ClassId(1),
    last: ClassId(1),
    parent: None,
    // SAFETY: as for INTERCEPTORS
    attributes: &[Attribute { name: "size", get: unsafe { Bound::host_fn(size) }, set: Some(unsafe { Bound::host_fn(set_size) }) }],
    operations: &[],
    constants: &[],
    interceptors: Some(&INTERCEPTORS),
    ops: Some(&OPS),
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
    let (_, h) = unsafe { Ctx::parts::<Stub, Log>(ctx) };
    Stub::to_bits(Stub::number(h.size))
}

pub unsafe extern "C" fn set_size(ctx: *mut Ctx, _this: u64, argv: *const u64, argc: usize) -> u64 {
    // SAFETY: called with a context for Log and its one argument
    let ((e, h), args) = unsafe { (Ctx::parts::<Stub, Log>(ctx), Stub::values(argv, argc)) };
    h.size = e.as_number(args[0]).unwrap();
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
