//! The engine trait, the host trait, and what passes between them.

use std::fmt::Debug;
use std::mem::{align_of, size_of};
use std::time::Instant;

use madcawl_str::{Atom, DomString};

use crate::class::{Bound, ClassDef, HostFn};
use crate::value::{ErrorKind, HostRef, Key, Owned, ValueKind};

/// One isolate of an engine, used on the thread it belongs to: every
/// method is a call into that isolate. Calls that can run script or
/// reach host code take the host, which is passed down to the host
/// functions they call.
///
/// An isolate is one agent cluster: the realms of the agents that can
/// share memory with each other. Several isolates may live on one
/// thread (a content process's pages, run in turn); each has its own
/// heap, cycle candidates, microtask queue, pending exception, script
/// depth and termination state, and no value or handle passes from one
/// to another.
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

    /// The value's word, as host functions take and return it.
    fn to_bits(v: Self::Value) -> u64;
    /// The value whose word is `bits`, which came from [`Engine::to_bits`].
    fn from_bits(bits: u64) -> Self::Value;
    /// The `argc` words at `argv` as values.
    ///
    /// # Safety
    /// `argv` points at `argc` words from [`Engine::to_bits`] that stay
    /// put for `'a`; it may be null when `argc` is 0.
    unsafe fn values<'a>(argv: *const u64, argc: usize) -> &'a [Self::Value];

    fn undefined() -> Self::Value;
    fn null() -> Self::Value;
    fn boolean(b: bool) -> Self::Value;
    fn number(n: f64) -> Self::Value;
    /// The sentinel a host function returns once it has registered an
    /// exception.
    fn thrown() -> Owned<Self::Value>;
    fn is_thrown(v: Self::Value) -> bool;
    /// The sentinel an index getter returns for no such index: neither a
    /// script value nor [`Engine::thrown`].
    fn absent() -> Self::Value;
    fn kind(&self, v: Self::Value) -> ValueKind;
    fn as_boolean(&self, v: Self::Value) -> Option<bool>;
    fn as_number(&self, v: Self::Value) -> Option<f64>;
    /// The string `v` is, as a flat block with a count for the caller:
    /// the same block when `v` is one, otherwise a new one (an engine may
    /// keep short strings in the value word, or a substring as a view
    /// into another block). The host tells strings by [`Engine::kind`] or
    /// by this, never by a block's type tag.
    fn as_string(&mut self, v: Self::Value) -> Option<DomString>;

    /// One more count on `v`, which the caller holds or was lent: its
    /// count is above 0. A wrapper read from a wrapper word is taken with
    /// [`Engine::wrapper_upgrade`] instead.
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
    /// A WindowProxy: the object script gets for a browsing context's
    /// window, forwarding every property access to the global object of
    /// `realm`. Which object the global's own accessors see as `this`
    /// (the proxy, as the web requires) is not settled here yet: it
    /// comes with the global as a host object.
    fn new_window_proxy(&mut self, realm: Self::Realm) -> Owned<Self::Value>;
    /// Navigation: `proxy` forwards to the global of `realm` from now on.
    /// Script that holds the proxy sees the new global through the same
    /// object; the old realm is detached by the host as usual.
    fn set_window_proxy_target(&mut self, proxy: Self::Value, realm: Self::Realm);
    /// The realm `proxy` forwards to; `None` when `proxy` is no WindowProxy.
    fn window_proxy_target(&self, proxy: Self::Value) -> Option<Self::Realm>;
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
    /// Call `f`. Thrown when it threw, with the exception registered, or
    /// when script is being ended ([`Engine::is_terminating`]), with
    /// none registered.
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
        f: Bound<HostFn, Self, H>,
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
    /// The wrapper whose word is `word` (preserved bit clear), with a
    /// count for the caller, as `Weak::upgrade` does: `None` once its
    /// count reached 0 (its `wrapper_died` notice may still be queued) or
    /// once the collector found it to be garbage, even while its cycle is
    /// still being unlinked. On `None` the host clears the wrapper word,
    /// gives up the count the wrapper held on the host object (and the
    /// preserved edge's count, if the bit was set), and makes a new
    /// wrapper; the engine sends no `wrapper_died` for the old one.
    ///
    /// The engine does not reuse a wrapper's memory before its notice is
    /// delivered by [`Engine::drain_host`] or refused here, so a word the
    /// host still holds is always safe to pass.
    fn wrapper_upgrade(&mut self, word: u64) -> Option<Owned<Self::Value>>;
    /// The wrapper whose word is `word` (preserved bit clear), borrowed,
    /// for a word whose preserved bit is set: `traverse` reports the
    /// preserved edge with it, and `unlink` gives back that edge's count.
    /// No engine call and no liveness check; reading a wrapper for
    /// script goes through [`Engine::wrapper_upgrade`].
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
    /// hand, and free those wrappers. How many were delivered.
    fn drain_host<H: Host<Self>>(&mut self, host: &mut H) -> usize;

    fn weak(&mut self, v: Self::Value) -> Self::Weak;
    fn weak_get(&mut self, w: &Self::Weak) -> Option<Owned<Self::Value>>;
    fn weak_drop(&mut self, w: Self::Weak);

    /// `obj`'s count dropped and stayed above zero while it is outside
    /// every active document: it may close a cycle. Every such decrement
    /// must be registered, wherever it happens (a child moved out, a
    /// parent left behind by a move, a template's contents when the
    /// template is freed); a decrement not registered can leave a cycle
    /// no collection ever looks at. Registering again is harmless.
    fn suspect(&mut self, obj: HostRef);
    /// `obj` was freed: drop it from the candidates and from a
    /// collection in progress.
    fn forget(&mut self, obj: HostRef);
    /// Advance the cycle collection until `deadline`.
    ///
    /// A collection builds its graph in batches, decides in one step
    /// (the verdict, not divisible), and unlinks the garbage in batches;
    /// script runs between batches. In the step that decides, every
    /// wrapper in the garbage set is marked dead, so that from then on
    /// [`Engine::wrapper_upgrade`] refuses it: script reading a node's
    /// wrapper between two unlink batches gets a new wrapper, and what it
    /// writes there is not taken by a later batch. A host that calls this
    /// directly calls [`Engine::drain_host`] (and frees what that lets
    /// go) before and after.
    ///
    /// Between unlink batches the host must not make a wrapper for a
    /// node it reaches by a raw id (one no count records) when that node
    /// is in the garbage set: such a node is only reachable that way, and
    /// a wrapper made for it would be unlinked with it. Holding a node
    /// across calls takes a counted node handle, which keeps it out of
    /// the garbage set.
    fn collect_cycles<H: Host<Self>>(&mut self, host: &mut H, deadline: Deadline) -> Step;

    /// Queue `job(arg)`; `job` is callable (a host function for a host
    /// microtask).
    fn enqueue_microtask(&mut self, job: Owned<Self::Value>, arg: Owned<Self::Value>);
    /// Run microtasks until the queue is empty. The host calls it at
    /// each checkpoint: after a call into script returns with
    /// [`Engine::script_depth`] 0, and at the end of every task; never
    /// while script is on the stack (a listener of an event script
    /// dispatched runs with no checkpoint after it).
    ///
    /// - A checkpoint entered while one runs returns at once.
    /// - Microtasks queued while draining run in the same drain.
    /// - A microtask's exception goes to [`Host::report_exception`] and
    ///   the draining goes on.
    /// - When script is ended during the drain ([`Engine::is_terminating`])
    ///   the draining stops, and nothing is reported; what is left stays
    ///   queued for the next checkpoint.
    fn run_microtasks<H: Host<Self>>(&mut self, host: &mut H);

    /// How many script and host functions are on the stack: 0 when the
    /// host is not inside a call into script. The engine counts every
    /// entry into a script function or a host function.
    fn script_depth(&self) -> u32;
    /// The engine is ending the running script on a `terminate` request:
    /// calls return thrown with no exception registered (one script had
    /// registered is dropped, and `throw` registers none), and the host
    /// reports nothing. It holds until the host next enters the isolate
    /// with [`Engine::script_depth`] 0 (a call, a property access, a
    /// checkpoint, an evaluation), which starts afresh; so a host that
    /// saw a call end this way skips that task's checkpoint.
    fn is_terminating(&self) -> bool;

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
