//! The engine interface on JavaScriptCore.

use madcawl_js::{
    Bound, ClassDef, Deadline, Engine, ErrorKind, Halt, Host, HostFn, HostRef, Key, Owned, ScriptOrigin, Step,
    Unsupported, ValueKind,
};
use madcawl_str::{Atom, DomString, HeldAtoms};

use crate::collect::{JscTracer, JscUnlinker};
use crate::ffi;
use crate::isolate::{Jsc, Realm};
use crate::objects::{FnKind, FnRec, make_function, thunk};
use crate::protos::Helper;
use crate::value::{ABSENT, FALSE, NULL, THROWN, TRUE, UNDEFINED, Value};

impl Engine for Jsc {
    type Value = Value;
    type Realm = Realm;
    type Weak = u32;
    type Tracer = JscTracer;
    type Unlinker = JscUnlinker;
    type ModulePayload = ();

    fn to_bits(v: Value) -> u64 {
        v.0
    }

    fn from_bits(bits: u64) -> Value {
        Value(bits)
    }

    unsafe fn values<'a>(argv: *const u64, argc: usize) -> &'a [Value] {
        if argc == 0 {
            return &[];
        }
        // SAFETY: the caller's; `Value` is a transparent word
        unsafe { std::slice::from_raw_parts(argv.cast(), argc) }
    }

    fn undefined() -> Value {
        UNDEFINED
    }

    fn null() -> Value {
        NULL
    }

    fn boolean(b: bool) -> Value {
        if b { TRUE } else { FALSE }
    }

    fn number(n: f64) -> Value {
        Value::number(n)
    }

    fn thrown() -> Owned<Value> {
        Jsc::owned(THROWN)
    }

    fn is_thrown(v: Value) -> bool {
        v == THROWN
    }

    fn absent() -> Value {
        ABSENT
    }

    fn kind(&self, v: Value) -> ValueKind {
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

    fn as_boolean(&self, v: Value) -> Option<bool> {
        v.as_boolean()
    }

    fn as_number(&self, v: Value) -> Option<f64> {
        v.as_number()
    }

    fn as_string(&mut self, v: Value) -> Option<DomString> {
        self.string_value(v)
    }

    fn retain(&mut self, v: Value) -> Owned<Value> {
        self.take(v)
    }

    fn release(&mut self, v: Owned<Value>) {
        self.unprotect(v.into_raw());
    }

    fn string(&mut self, s: DomString) -> Owned<Value> {
        Jsc::owned(self.string_of(&s))
    }

    fn to_atom(&mut self, v: Value, held: &HeldAtoms) -> Option<Atom> {
        let mut b = Vec::new();
        self.string_value(v)?.write_wtf8(&mut b);
        Some(held.hold(&b))
    }

    fn to_string<H: Host<Jsc>>(&mut self, host: &mut H, v: Value) -> Option<DomString> {
        self.convert_string(host, v)
    }

    fn to_number<H: Host<Jsc>>(&mut self, host: &mut H, v: Value) -> Option<f64> {
        self.convert_number(host, v)
    }

    fn to_boolean(&self, v: Value) -> bool {
        match v {
            UNDEFINED | NULL | FALSE => false,
            TRUE => true,
            // SAFETY: a live value; ToBoolean runs no script
            _ => v.as_number().map_or_else(|| unsafe { ffi::JSValueToBoolean(self.base, v.raw()) }, |n| n != 0.0 && !n.is_nan()),
        }
    }

    fn new_realm(&mut self) -> Realm {
        self.make_realm()
    }

    fn new_window_proxy(&mut self, realm: Realm) -> Owned<Value> {
        self.window_proxy(realm)
    }

    fn set_window_proxy_target(&mut self, proxy: Value, realm: Realm) {
        self.retarget(proxy, realm);
    }

    fn window_proxy_target(&self, proxy: Value) -> Option<Realm> {
        self.proxies.get(&proxy.0).map(|&(_, r)| r)
    }

    fn global(&mut self, realm: Realm) -> Owned<Value> {
        let ctx = self.realm_ctx(realm);
        // SAFETY: a live realm
        let g = Value(unsafe { ffi::JSContextGetGlobalObject(ctx) } as u64);
        self.take(g)
    }

    fn detach_realm(&mut self, realm: Realm) {
        let i = self.index(realm);
        crate::protos::detach(self, i);
    }

    fn new_object(&mut self, realm: Realm) -> Owned<Value> {
        let ctx = self.realm_ctx(realm);
        // SAFETY: a live realm
        let o = Value(unsafe { ffi::JSObjectMake(ctx, std::ptr::null_mut(), std::ptr::null_mut()) } as u64);
        self.take(o)
    }

    fn get<H: Host<Jsc>>(&mut self, host: &mut H, obj: Value, key: Key<'_>) -> Owned<Value> {
        self.get_prop(host, obj, key)
    }

    fn set<H: Host<Jsc>>(&mut self, host: &mut H, obj: Value, key: Key<'_>, v: Value) -> bool {
        self.set_prop(host, obj, key, v)
    }

    fn delete<H: Host<Jsc>>(&mut self, host: &mut H, obj: Value, key: Key<'_>) -> bool {
        self.delete_prop(host, obj, key)
    }

    fn is_callable(&self, v: Value) -> bool {
        self.callable(v)
    }

    fn call<H: Host<Jsc>>(&mut self, host: &mut H, f: Value, this: Value, args: &[Value]) -> Owned<Value> {
        self.call_value(host, f, this, args)
    }

    fn new_function<H: Host<Jsc>>(&mut self, realm: Realm, name: &'static str, length: u8, f: Bound<HostFn, Jsc, H>, data: u64) -> Owned<Value> {
        let ctx = self.realm_ctx(realm);
        let fproto = self.helper(realm.0, Helper::FunctionProto);
        let kind = FnKind::Host { f: f.get(), data, thunk: thunk::<H> };
        let v = make_function(self, ctx, FnRec { kind, host_type: std::any::TypeId::of::<H>() }, fproto);
        let n = self.string_of(&DomString::from_wtf8(name.as_bytes()));
        let r = self.call_helper(realm.0, Helper::Named, &[v, n, Value::number(f64::from(length))]);
        assert!(r.is_ok(), "naming a host function");
        self.unprotect(n);
        Jsc::owned(v)
    }

    fn interface_object<H: Host<Jsc>>(&mut self, realm: Realm, class: &'static ClassDef<Jsc, H>) -> Owned<Value> {
        let i = self.index(realm);
        let (iface, _) = self.class_objects(i, class);
        self.take(iface)
    }

    fn throw(&mut self, exception: Owned<Value>) -> Owned<Value> {
        let x = exception.into_raw();
        if !self.halted() {
            self.register(x);
        }
        self.unprotect(x);
        Jsc::owned(THROWN)
    }

    fn throw_error(&mut self, kind: ErrorKind, message: &str) -> Owned<Value> {
        let e = self.make_error(kind, message);
        self.throw(Jsc::owned(e))
    }

    fn take_exception(&mut self) -> Option<Owned<Value>> {
        self.exception.take().map(Jsc::owned)
    }

    fn new_wrapper<H: Host<Jsc>>(&mut self, realm: Realm, class: &'static ClassDef<Jsc, H>, obj: HostRef) -> Owned<Value> {
        self.make_wrapper(realm, class, obj)
    }

    fn wrapper_word(wrapper: Value) -> u64 {
        Jsc::word_of(wrapper)
    }

    fn wrapper_upgrade(&mut self, word: u64) -> Option<Owned<Value>> {
        self.upgrade(word)
    }

    fn wrapper_value(word: u64) -> Value {
        // SAFETY: the record of a preserved wrapper, which is alive
        unsafe { (*(word as *const crate::objects::WrapRec)).js }
    }

    fn host_of<H: Host<Jsc>>(&self, v: Value, class: &'static ClassDef<Jsc, H>) -> Option<HostRef> {
        self.host_object(v, class)
    }

    fn drain_host<H: Host<Jsc>>(&mut self, host: &mut H) -> usize {
        self.drain(host)
    }

    fn weak(&mut self, v: Value) -> u32 {
        self.weak_slot(v)
    }

    fn weak_get(&mut self, w: &u32) -> Option<Owned<Value>> {
        self.weak_read(*w)
    }

    fn weak_drop(&mut self, w: u32) {
        self.weak_slot_drop(w);
    }

    fn suspect(&mut self, obj: HostRef) {
        self.add_suspect(obj);
    }

    fn forget(&mut self, obj: HostRef) {
        self.forget_host(obj);
    }

    fn collect_cycles<H: Host<Jsc>>(&mut self, host: &mut H, deadline: Deadline) -> Step {
        self.collect_host(host, deadline)
    }

    fn enqueue_microtask(&mut self, job: Owned<Value>, arg: Owned<Value>) {
        // the host type is only known to calls that take the host: the
        // trampoline finds it in the frame of the call that runs it
        self.queue_job(job, arg);
    }

    fn run_microtasks<H: Host<Jsc>>(&mut self, host: &mut H) {
        self.checkpoint(host);
    }

    fn in_script(&self) -> bool {
        self.depth != 0
    }

    fn halt(&self) -> Halt {
        self.halt
    }

    fn resume(&mut self) -> Result<Halt, Unsupported> {
        if self.depth == 0 && self.halt == Halt::Aborted {
            self.halt = Halt::Running;
        }
        let bits = self.flags.bits.load(std::sync::atomic::Ordering::Acquire);
        if bits & crate::halt::TERMINATE != 0 {
            self.halt = Halt::Terminated;
        }
        Ok(self.halt)
    }

    fn discard_microtasks(&mut self) -> Result<(), Unsupported> {
        for v in std::mem::take(&mut self.pending) {
            self.unprotect(v);
        }
        Ok(())
    }

    fn evaluate<H: Host<Jsc>>(&mut self, host: &mut H, realm: Realm, source: &DomString, origin: &ScriptOrigin<'_>) -> Owned<Value> {
        self.eval_script(host, realm, source, origin)
    }

    fn compile_handler<H: Host<Jsc>>(
        &mut self,
        host: &mut H,
        realm: Realm,
        params: &[&str],
        body: &DomString,
        scopes: &[Value],
        _origin: &ScriptOrigin<'_>,
    ) -> Owned<Value> {
        self.handler(host, realm, params, body, scopes)
    }

    fn finish_loading_module<H: Host<Jsc>>(&mut self, _host: &mut H, _payload: (), result: Owned<Value>) {
        self.release(result);
    }
}
