//! The engine interface on the stub.

use madcawl_js::{
    Bound, ClassDef, Deadline, Engine, ErrorKind, Halt, Host, HostFn, HostRef, Key, Owned,
    ScriptOrigin, Step, Unsupported, ValueKind,
};
use madcawl_str::{Atom, DomString, HeldAtoms};

use crate::cell::{Body, Cell, ErasedClass};
use crate::collect::{Phase, StubTracer, StubUnlinker, Vertex};
use crate::isolate::{Realm, Stub};
use crate::props::{class_as, detach};
use crate::value::{ABSENT, FALSE, NULL, THROWN, TRUE, UNDEFINED, Value};

const NO_SCRIPT: &str = "the stub engine runs no script";

impl Engine for Stub {
    type Value = Value;
    type Realm = Realm;
    type Weak = u32;
    type Tracer = StubTracer;
    type Unlinker = StubUnlinker;
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
        Stub::owned(THROWN)
    }

    fn is_thrown(v: Value) -> bool {
        v == THROWN
    }

    fn absent() -> Value {
        ABSENT
    }

    fn kind(&self, v: Value) -> ValueKind {
        match v {
            UNDEFINED => ValueKind::Undefined,
            NULL => ValueKind::Null,
            TRUE | FALSE => ValueKind::Boolean,
            _ if v.as_number().is_some() => ValueKind::Number,
            _ if v.is_string() || self.is_view(v) => ValueKind::String,
            _ if self.is_symbol(v) => ValueKind::Symbol,
            _ if self.is_callable(v) => ValueKind::Function,
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
        self.flat_string(v)
    }

    fn retain(&mut self, v: Value) -> Owned<Value> {
        self.retain_value(v);
        Stub::owned(v)
    }

    fn release(&mut self, v: Owned<Value>) {
        self.release_value(v.into_raw());
    }

    fn string(&mut self, s: DomString) -> Owned<Value> {
        Stub::owned(Value(s.into_raw().as_ptr() as u64))
    }

    fn to_atom(&mut self, v: Value, held: &HeldAtoms) -> Option<Atom> {
        let mut b = Vec::new();
        self.as_string(v)?.write_wtf8(&mut b);
        Some(held.hold(&b))
    }

    fn to_string<H: Host<Stub>>(&mut self, host: &mut H, v: Value) -> Option<DomString> {
        self.string_of(host, v)
    }

    fn to_number<H: Host<Stub>>(&mut self, host: &mut H, v: Value) -> Option<f64> {
        self.number_of(host, v)
    }

    fn to_boolean(&self, v: Value) -> bool {
        self.truthy(v)
    }

    fn new_realm(&mut self) -> Realm {
        self.make_realm()
    }

    fn global(&mut self, realm: Realm) -> Owned<Value> {
        let g = self.live_global(self.index(realm));
        self.retain(g)
    }

    fn detach_realm(&mut self, realm: Realm) {
        detach(self, realm);
    }

    fn live_realms(&self) -> usize {
        self.realms.iter().filter(|r| !r.detached).count()
    }

    fn new_object(&mut self, realm: Realm) -> Owned<Value> {
        Stub::owned(self.alloc(self.index(realm), Body::Object))
    }

    fn new_array(&mut self, realm: Realm, items: &[Value]) -> Owned<Value> {
        self.array(realm, items)
    }

    fn get<H: Host<Stub>>(&mut self, host: &mut H, obj: Value, key: Key<'_>) -> Owned<Value> {
        self.get_prop(host, obj, key)
    }

    fn set<H: Host<Stub>>(&mut self, host: &mut H, obj: Value, key: Key<'_>, v: Value) -> bool {
        self.set_prop(host, obj, key, v)
    }

    fn delete<H: Host<Stub>>(&mut self, _host: &mut H, obj: Value, key: Key<'_>) -> bool {
        self.delete_prop(obj, key)
    }

    fn is_callable(&self, v: Value) -> bool {
        // SAFETY: a live cell, read only
        v.is_cell() && matches!(unsafe { Cell::at(v) }.body, Body::Function { .. })
    }

    fn call<H: Host<Stub>>(&mut self, host: &mut H, f: Value, this: Value, args: &[Value]) -> Owned<Value> {
        self.call_value(host, f, this, args)
    }

    fn new_function<H: Host<Stub>>(&mut self, realm: Realm, name: &'static str, length: u8, f: Bound<HostFn, Stub, H>, data: u64) -> Owned<Value> {
        self.function::<H>(self.index(realm), name, length, f, data)
    }

    fn interface_object<H: Host<Stub>>(&mut self, realm: Realm, class: &'static ClassDef<Stub, H>) -> Owned<Value> {
        let (iface, _) = self.class_objects(self.index(realm), class);
        self.retain(iface)
    }

    fn throw(&mut self, exception: Owned<Value>) -> Owned<Value> {
        // halted script registers nothing
        if self.halted() {
            self.release(exception);
            return Stub::thrown();
        }
        if let Some(old) = self.exception.replace(exception.into_raw()) {
            self.release_value(old);
        }
        Stub::thrown()
    }

    fn throw_error(&mut self, kind: ErrorKind, message: &str) -> Owned<Value> {
        let e = self.error(kind, message);
        self.throw(e)
    }

    fn take_exception(&mut self) -> Option<Owned<Value>> {
        self.exception.take().map(Stub::owned)
    }

    fn new_wrapper<H: Host<Stub>>(&mut self, realm: Realm, class: &'static ClassDef<Stub, H>, obj: HostRef) -> Owned<Value> {
        // script could reach a garbage host object only through this
        // wrapper, and a later unlink batch would take what it writes there
        if let Phase::Unlink { garbage, .. } = &self.collector.phase {
            assert!(!garbage.contains(&Vertex::Host(obj)), "a wrapper for a host object found to be garbage");
        }
        let class = ErasedClass::of(class);
        let body = Body::Wrapper { class, obj, preserved: false, dead: false, condemned: false, orphan: false };
        Stub::owned(self.alloc(self.index(realm), body))
    }

    fn wrapper_word(wrapper: Value) -> u64 {
        wrapper.0
    }

    fn wrapper_upgrade(&mut self, word: u64) -> Option<Owned<Value>> {
        let v = Value(word);
        // SAFETY: a wrapper the host holds the word of is not freed before
        // its notice is delivered or it is refused here
        let c = unsafe { Cell::at(v) };
        self.own(c);
        let Body::Wrapper { dead, condemned, orphan, .. } = &mut c.body else {
            panic!("a wrapper word names a wrapper")
        };
        assert!(!*orphan, "a refused wrapper's word was cleared");
        if *dead || *condemned {
            *orphan = true;
            return None;
        }
        Some(self.retain(v))
    }

    fn wrapper_value(word: u64) -> Value {
        Value(word)
    }

    fn host_of<H: Host<Stub>>(&self, v: Value, class: &'static ClassDef<Stub, H>) -> Option<HostRef> {
        if !v.is_cell() {
            return None;
        }
        // SAFETY: a live cell, read only
        match unsafe { Cell::at(v) }.body {
            Body::Wrapper { class: c, obj, .. } => {
                (c.host == std::any::TypeId::of::<H>() && class.contains(c.id)).then_some(obj)
            }
            _ => None,
        }
    }

    fn drain_host<H: Host<Stub>>(&mut self, host: &mut H) -> usize {
        let mut n = 0;
        while let Some(v) = self.notices.pop() {
            // SAFETY: a noticed cell is not freed before its notice
            let c = unsafe { Cell::at(v) };
            let Body::Wrapper { class, obj, orphan, .. } = c.body else { unreachable!() };
            if !orphan {
                (class_as::<H>(&class).host_ops().wrapper_died)(host, obj);
                n += 1;
            }
            self.free_cell(v);
        }
        n
    }

    fn weak(&mut self, v: Value) -> u32 {
        self.weak_slot(v)
    }

    fn weak_get(&mut self, w: &u32) -> Option<Owned<Value>> {
        let v = self.weak[*w as usize]?;
        Some(self.retain(v))
    }

    fn weak_drop(&mut self, w: u32) {
        self.weak_slot_drop(w);
    }

    fn suspect(&mut self, obj: HostRef) {
        self.buffer_vertex(Vertex::Host(obj));
    }

    fn forget(&mut self, obj: HostRef) {
        self.unbuffer(Vertex::Host(obj));
    }

    fn collect_cycles<H: Host<Stub>>(&mut self, host: &mut H, deadline: Deadline) -> Step {
        self.collect(host, deadline)
    }

    fn enqueue_microtask(&mut self, job: Owned<Value>, arg: Owned<Value>) {
        self.queue_microtask(job.into_raw(), arg.into_raw());
    }

    fn run_microtasks<H: Host<Stub>>(&mut self, host: &mut H) {
        self.deliver(host);
        if std::mem::replace(&mut self.in_checkpoint, true) {
            return;
        }
        while !self.halted()
            && let Some((job, arg)) = self.microtasks.pop_front()
        {
            let r = if self.detached_job(job) { UNDEFINED } else { self.call_value(host, job, UNDEFINED, &[arg]).into_raw() };
            if r == THROWN && !self.halted() {
                let e = self.take_exception().expect("a thrown exception");
                host.report_exception(self, e);
            }
            for v in [r, job, arg] {
                self.release_value(v);
            }
        }
        self.in_checkpoint = false;
    }

    fn in_script(&self) -> bool {
        self.depth != 0
    }

    fn halt(&self) -> Halt {
        self.halt
    }

    fn resume(&mut self) -> Result<Halt, Unsupported> {
        self.resume_script();
        Ok(self.halt)
    }

    fn discard_microtasks(&mut self) -> Result<(), Unsupported> {
        // a termination emptied the queue and keeps it empty
        if self.halt != Halt::Terminated {
            self.drop_microtasks();
        }
        Ok(())
    }

    fn evaluate<H: Host<Stub>>(&mut self, _host: &mut H, _realm: Realm, _source: &DomString, _origin: &ScriptOrigin<'_>) -> Owned<Value> {
        self.poll_interrupt();
        if self.halted() {
            return Stub::thrown();
        }
        self.throw_error(ErrorKind::Syntax, NO_SCRIPT)
    }

    fn compile_handler<H: Host<Stub>>(
        &mut self,
        _host: &mut H,
        _realm: Realm,
        _params: &[&str],
        _body: &DomString,
        _scopes: &[Value],
        _origin: &ScriptOrigin<'_>,
    ) -> Owned<Value> {
        self.poll_interrupt();
        if self.halted() {
            return Stub::thrown();
        }
        self.throw_error(ErrorKind::Syntax, NO_SCRIPT)
    }

    fn finish_loading_module<H: Host<Stub>>(&mut self, _host: &mut H, _payload: (), result: Owned<Value>) {
        self.release(result);
    }
}
