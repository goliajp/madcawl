//! The engine trait, the host trait, and what passes between them.

use std::fmt::Debug;
use std::mem::{align_of, size_of};
use std::time::Instant;

use madcawl_str::{Atom, DomString};

use crate::class::{ClassDef, HostFn};
use crate::value::{ErrorKind, HostRef, Key, Owned, ValueKind};

/// One isolate of an engine, used on the thread it belongs to: every
/// method is a call into that isolate. Calls that can run script or
/// reach host code take the host, which is passed down to the host
/// functions they call.
pub trait Engine: Sized + 'static {
    /// A script value: 8 bytes, copied by its bits, opaque to the host.
    type Value: Copy + Eq + Debug;
    type Realm: Copy + Eq + Debug;
    /// A weak handle: no count; reads as gone once its target is freed.
    type Weak;
    type Tracer: Tracer<Self>;
    type Unlinker: Unlinker<Self>;
    /// What the engine needs back when a module the host loaded is
    /// ready (`FinishLoadingImportedModule`).
    type ModulePayload;

    /// Evaluated by the binding layer's constructor, so that an engine
    /// whose values are not one word does not compile.
    const VALUE_IS_ONE_WORD: () =
        assert!(size_of::<Self::Value>() == 8 && align_of::<Self::Value>() <= 8);

    fn undefined() -> Self::Value;
    fn null() -> Self::Value;
    fn boolean(b: bool) -> Self::Value;
    fn number(n: f64) -> Self::Value;
    /// The sentinel a host function returns once it has registered an
    /// exception.
    fn thrown() -> Owned<Self::Value>;
    fn is_thrown(v: Self::Value) -> bool;
    fn kind(&self, v: Self::Value) -> ValueKind;
    fn as_boolean(&self, v: Self::Value) -> Option<bool>;
    fn as_number(&self, v: Self::Value) -> Option<f64>;
    /// The string `v` is, with a new count on the same block.
    fn as_string(&self, v: Self::Value) -> Option<DomString>;

    fn retain(&mut self, v: Self::Value) -> Owned<Self::Value>;
    fn release(&mut self, v: Owned<Self::Value>);

    /// `s` as a value, taking its count: the block is not copied.
    fn string(&mut self, s: DomString) -> Owned<Self::Value>;
    /// ECMAScript ToString; `None` when it threw.
    fn to_string<H: Host<Self>>(&mut self, host: &mut H, v: Self::Value) -> Option<DomString>;
    /// ECMAScript ToNumber; `None` when it threw.
    fn to_number<H: Host<Self>>(&mut self, host: &mut H, v: Self::Value) -> Option<f64>;
    fn to_boolean(&self, v: Self::Value) -> bool;

    fn new_realm(&mut self) -> Self::Realm;
    fn global(&mut self, realm: Self::Realm) -> Owned<Self::Value>;
    /// Let the realm go: no more script runs in it, and what only it
    /// holds is left to the collector. Objects other realms hold stay.
    fn detach_realm(&mut self, realm: Self::Realm);

    fn new_object(&mut self, realm: Self::Realm) -> Owned<Self::Value>;
    fn get<H: Host<Self>>(
        &mut self,
        host: &mut H,
        obj: Self::Value,
        key: Key<'_>,
    ) -> Owned<Self::Value>;
    /// `false` when it threw. `v` is borrowed; the property takes its
    /// own count.
    fn set<H: Host<Self>>(
        &mut self,
        host: &mut H,
        obj: Self::Value,
        key: Key<'_>,
        v: Self::Value,
    ) -> bool;
    fn delete<H: Host<Self>>(&mut self, host: &mut H, obj: Self::Value, key: Key<'_>) -> bool;
    fn is_callable(&self, v: Self::Value) -> bool;
    fn call<H: Host<Self>>(
        &mut self,
        host: &mut H,
        f: Self::Value,
        this: Self::Value,
        args: &[Self::Value],
    ) -> Owned<Self::Value>;
    fn new_function<H: Host<Self>>(
        &mut self,
        realm: Self::Realm,
        name: &'static str,
        length: u8,
        f: HostFn<Self, H>,
    ) -> Owned<Self::Value>;

    /// Register `exception`; returns [`Engine::thrown`].
    fn throw(&mut self, exception: Owned<Self::Value>) -> Owned<Self::Value>;
    fn throw_error(&mut self, kind: ErrorKind, message: &str) -> Owned<Self::Value>;
    fn take_exception(&mut self) -> Option<Owned<Self::Value>>;

    /// A wrapper of class `class` for host object `obj`. The wrapper
    /// holds `obj`: the host counts that edge when it stores the
    /// wrapper's word in `obj`.
    fn new_wrapper<H: Host<Self>>(
        &mut self,
        realm: Self::Realm,
        class: &'static ClassDef<Self, H>,
        obj: HostRef,
    ) -> Owned<Self::Value>;
    /// The word a host stores for `wrapper`; its low bit is clear.
    fn wrapper_word(wrapper: Self::Value) -> u64;
    /// The wrapper whose word is `word` (preserved bit clear), borrowed:
    /// valid while the host object holds the word. A host that keeps it
    /// takes a count with [`Engine::retain`]. No engine call: `traverse`
    /// reports a preserved wrapper with it.
    fn wrapper_value(word: u64) -> Self::Value;
    /// The host object of `v` if `v` is a wrapper of `class` or of a
    /// class derived from it.
    fn host_of<H: Host<Self>>(
        &self,
        v: Self::Value,
        class: &'static ClassDef<Self, H>,
    ) -> Option<HostRef>;
    /// Deliver the notices kept for the host (`wrapper_died`), which
    /// arise wherever a count drops and are held until the host is at
    /// hand. How many were delivered.
    fn drain_host<H: Host<Self>>(&mut self, host: &mut H) -> usize;

    fn weak(&mut self, v: Self::Value) -> Self::Weak;
    fn weak_get(&mut self, w: &Self::Weak) -> Option<Owned<Self::Value>>;
    fn weak_drop(&mut self, w: Self::Weak);

    /// `obj`'s count dropped and stayed above zero while it is outside
    /// every active document: it may close a cycle.
    fn suspect(&mut self, obj: HostRef);
    /// `obj` was freed: drop it from the candidates and from a
    /// collection in progress.
    fn forget(&mut self, obj: HostRef);
    /// Advance the cycle collection until `deadline`.
    fn collect_cycles<H: Host<Self>>(&mut self, host: &mut H, deadline: Deadline) -> Step;

    /// Queue `job(arg)`; `job` is callable (a host function for a host
    /// microtask).
    fn enqueue_microtask(&mut self, job: Owned<Self::Value>, arg: Owned<Self::Value>);
    /// Run microtasks until the queue is empty. The host calls it at
    /// each checkpoint: after a call into script returns with the stack
    /// empty, and at the end of every task. Exceptions go to
    /// [`Host::report_exception`] and the draining goes on.
    fn run_microtasks<H: Host<Self>>(&mut self, host: &mut H);

    /// Compile and run a classic script.
    fn evaluate<H: Host<Self>>(
        &mut self,
        host: &mut H,
        realm: Self::Realm,
        source: &DomString,
        origin: &ScriptOrigin<'_>,
    ) -> Owned<Self::Value>;
    /// An event handler attribute's function: `body` compiled with
    /// `params` in the scope chain `scopes` (element, form, document).
    fn compile_handler<H: Host<Self>>(
        &mut self,
        host: &mut H,
        realm: Self::Realm,
        params: &[&str],
        body: &DomString,
        scopes: &[Self::Value],
        origin: &ScriptOrigin<'_>,
    ) -> Owned<Self::Value>;
    /// The module the host was asked for is ready, or failed
    /// (`result` thrown).
    fn finish_loading_module<H: Host<Self>>(
        &mut self,
        host: &mut H,
        payload: Self::ModulePayload,
        result: Owned<Self::Value>,
    );
}

/// The embedder's side: what the engine calls back.
pub trait Host<E: Engine>: Sized + 'static {
    /// The most derived class of host object `obj`.
    fn class_of(&self, obj: HostRef) -> &'static ClassDef<E, Self>;

    /// An exception no caller catches (a microtask's, a listener's).
    fn report_exception(&mut self, e: &mut E, exception: Owned<E::Value>) {
        e.release(exception);
    }

    /// `HostEnsureCanCompileStrings`: content security policy.
    fn can_compile_strings(&mut self, realm: E::Realm) -> bool {
        let _ = realm;
        true
    }

    /// `HostLoadImportedModule`: the module map belongs to the host,
    /// which fetches and then calls [`Engine::finish_loading_module`].
    fn load_imported_module(
        &mut self,
        e: &mut E,
        request: &ModuleRequest<'_>,
        payload: E::ModulePayload,
    );

    /// `HostPromiseRejectionTracker`: `handled` false for "reject",
    /// true for "handle".
    fn promise_rejection(&mut self, e: &mut E, promise: E::Value, handled: bool) {
        let _ = (e, promise, handled);
    }
}

/// What `traverse` reports to. Each call is one counted edge.
pub trait Tracer<E: Engine> {
    /// An edge to another host object.
    fn host(&mut self, obj: HostRef);
    /// An edge to a script value: a member handle, a preserved wrapper.
    fn value(&mut self, v: E::Value);
}

/// What `unlink` gives its edges up to. The engine releases them when
/// the batch is over, so every object of the cycle stays readable until
/// then.
pub trait Unlinker<E: Engine> {
    fn release(&mut self, v: Owned<E::Value>);
}

#[derive(Clone, Copy, Debug)]
pub enum Deadline {
    /// Stop after this many units of work (an object visited or
    /// unlinked): deterministic, for tests and for budgets.
    Work(u32),
    At(Instant),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The collection is not finished; call again.
    More,
    /// Nothing more to do until new candidates come.
    Done,
}

pub struct ScriptOrigin<'a> {
    pub url: &'a DomString,
    pub line: u32,
    pub column: u32,
}

pub struct ModuleRequest<'a> {
    pub referrer: Option<&'a DomString>,
    pub specifier: &'a DomString,
    /// The `type` import attribute.
    pub module_type: Option<Atom>,
}

/// What the host may do from any thread, and the memory limits.
pub trait Control {
    type Interrupt: Interrupt;
    fn interrupt_handle(&self) -> Self::Interrupt;
    /// Past `soft` the engine calls back the host; past `hard` it ends
    /// the running script uncatchably.
    fn set_memory_limits(&mut self, soft: usize, hard: usize);
    /// Memory host objects keep alive (nodes, attributes, decoded
    /// images), counted against the limits.
    fn adjust_external_memory(&mut self, delta: isize);
    fn memory_usage(&self) -> usize;
}

/// Sent to another thread to stop or call back running script. The
/// engine answers within its interrupt bound in script and its own
/// builtins; host functions are the host's to keep short.
pub trait Interrupt: Send + Sync + Clone {
    fn terminate(&self);
    fn request_callback(&self);
}
