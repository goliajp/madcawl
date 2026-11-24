//! The engine interface on the stub.

use std::any::TypeId;

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

    fn to_string<H: Host<Stub>>(&mut self, _host: &mut H, v: Value) -> Option<DomString> {
        if let Some(s) = self.as_string(v) {
            return Some(s);
        }
        let text = match self.kind(v) {
            ValueKind::Undefined => "undefined".to_owned(),
            ValueKind::Null => "null".to_owned(),
            ValueKind::Boolean => (v == TRUE).to_string(),
            ValueKind::Number => number_text(v.as_number().expect("a number")),
            ValueKind::Function => "function () { [native code] }".to_owned(),
            _ => "[object Object]".to_owned(),
        };
        Some(DomString::from_wtf8(text.as_bytes()))
    }

    fn to_number<H: Host<Stub>>(&mut self, _host: &mut H, v: Value) -> Option<f64> {
        Some(match self.kind(v) {
            ValueKind::Number => v.as_number().expect("a number"),
            ValueKind::Boolean => (v == TRUE) as u8 as f64,
            ValueKind::Null => 0.0,
            ValueKind::String => {
                let mut b = Vec::new();
                self.as_string(v).expect("a string").write_wtf8(&mut b);
                let t = String::from_utf8_lossy(&b);
                let t = t.trim();
                if t.is_empty() { 0.0 } else { t.parse().unwrap_or(f64::NAN) }
            }
            _ => f64::NAN,
        })
    }

    fn to_boolean(&self, v: Value) -> bool {
        match self.kind(v) {
            ValueKind::Undefined | ValueKind::Null => false,
            ValueKind::Boolean => v == TRUE,
            ValueKind::Number => v.as_number().is_some_and(|n| n != 0.0 && !n.is_nan()),
            ValueKind::String => self.string_len(v).is_some_and(|n| n != 0),
            _ => true,
        }
    }

    fn new_realm(&mut self) -> Realm {
        self.make_realm()
    }

    fn global(&mut self, realm: Realm) -> Owned<Value> {
        let g = self.live_global(self.index(realm));
        self.retain(g)
    }

    fn new_window_proxy(&mut self, realm: Realm) -> Owned<Value> {
        self.window_proxy(realm)
    }

    fn set_window_proxy_target(&mut self, proxy: Value, realm: Realm) {
        self.retarget(proxy, realm);
    }

    fn window_proxy_target(&self, proxy: Value) -> Option<Realm> {
        self.proxy_realm(proxy)
    }

    fn detach_realm(&mut self, realm: Realm) {
        detach(self, realm);
    }

    fn new_object(&mut self, realm: Realm) -> Owned<Value> {
        Stub::owned(self.alloc(self.index(realm), Body::Object))
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

    fn new_function<H: Host<Stub>>(&mut self, realm: Realm, name: &'static str, length: u8, f: Bound<HostFn, Stub, H>) -> Owned<Value> {
        self.function::<H>(self.index(realm), name, length, f)
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
        let class = ErasedClass {
            def: std::ptr::from_ref(class).cast(),
            host: TypeId::of::<H>(),
            id: class.id,
        };
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
                (c.host == TypeId::of::<H>() && class.contains(c.id)).then_some(obj)
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
        assert!(v.is_cell(), "a weak handle to an object");
        let slot = match self.weak_free.pop() {
            Some(s) => {
                self.weak[s as usize] = Some(v);
                s
            }
            None => {
                self.weak.push(Some(v));
                (self.weak.len() - 1) as u32
            }
        };
        self.weak_of.entry(v).or_default().push(slot);
        slot
    }

    fn weak_get(&mut self, w: &u32) -> Option<Owned<Value>> {
        let v = self.weak[*w as usize]?;
        Some(self.retain(v))
    }

    fn weak_drop(&mut self, w: u32) {
        if let Some(v) = self.weak[w as usize].take()
            && let Some(slots) = self.weak_of.get_mut(&v)
        {
            slots.retain(|&s| s != w);
        }
        self.weak_free.push(w);
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
        self.poll_interrupt();
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

    fn resume(&mut self) -> Result<(), Unsupported> {
        self.resume_script();
        Ok(())
    }

    fn discard_microtasks(&mut self) -> Result<(), Unsupported> {
        self.drop_microtasks();
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

fn number_text(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_owned()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity" } else { "-Infinity" }.to_owned()
    } else if n == n.trunc() && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

