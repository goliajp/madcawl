//! The engine trait, the host trait, and what passes between them.

use std::fmt::Debug;
use std::mem::{align_of, size_of};

use madcawl_str::{Atom, DomString, HeldAtoms};

use crate::class::{Bound, ClassDef, HostFn};
use crate::host::Host;
use crate::graph::{Deadline, Step, Tracer, Unlinker};
use crate::halt::{Control, Halt, Unsupported};
use crate::script::ScriptOrigin;
use crate::value::{ErrorKind, HostRef, Key, Owned, Tree, ValueKind};

/// One isolate of an engine, used on the thread it belongs to: every
/// method is a call into that isolate. Calls that can run script or
/// reach host code take the host, which is passed down to the host
/// functions they call.
///
/// An isolate is one agent cluster: the realms of the agents that can
/// share memory with each other. Several isolates may live on one
/// thread (a content process's pages, run in turn); each has its own
/// heap, cycle candidates, microtask queue, pending exception, whether
/// the host is inside a call, and halt state, and no value, realm or
/// handle passes from one to another.
///
/// [`Control`] is what the host does from other threads (interrupts)
/// and the memory limits; every engine has it.
pub trait Engine: Control + Sized + 'static {
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
    /// The atom that spells the string `v` is, held by `held` (the
    /// caller's holder, such as its document's, so that names script
    /// makes up go when it does); `None` when `v` is no string. Names
    /// (tag and attribute names, event types) take this path, not
    /// [`Engine::as_string`]: it makes no block for a short string kept
    /// in the value word.
    fn to_atom(&mut self, v: Self::Value, held: &HeldAtoms) -> Option<Atom>;
    /// ECMAScript ToString; `None` when it threw.
    fn to_string<H: Host<Self>>(&mut self, host: &mut H, v: Self::Value) -> Option<DomString>;
    /// ECMAScript ToNumber; `None` when it threw.
    fn to_number<H: Host<Self>>(&mut self, host: &mut H, v: Self::Value) -> Option<f64>;
    fn to_boolean(&self, v: Self::Value) -> bool;

    fn new_realm(&mut self) -> Self::Realm;
    /// A WindowProxy: the object script gets for a browsing context's
    /// window, forwarding every property access to the global object of
    /// `realm`; thrown, with nothing registered, when script is halted
    /// and the engine needs script to make one. What `this` the global's
    /// own accessors see is settled with the global as a host object.
    fn new_window_proxy(&mut self, realm: Self::Realm) -> Owned<Self::Value>;
    /// Navigation: `proxy` forwards to the global of `realm` from now on.
    /// Script that holds the proxy sees the new global through the same
    /// object; the old realm is detached by the host as usual.
    ///
    /// `realm` is one of this isolate's. A window in another agent
    /// cluster (a cross-site iframe, an opener) is never a target: the
    /// host gives script a host object that stands for it instead.
    fn set_window_proxy_target(&mut self, proxy: Self::Value, realm: Self::Realm);
    /// The realm `proxy` forwards to; `None` when `proxy` is no WindowProxy.
    fn window_proxy_target(&self, proxy: Self::Value) -> Option<Self::Realm>;
    fn global(&mut self, realm: Self::Realm) -> Owned<Self::Value>;
    /// Let the realm go: no more script runs in it, and what only it
    /// holds is left to the collector. Objects other realms hold stay.
    fn detach_realm(&mut self, realm: Self::Realm);
    /// How many realms are not detached.
    fn live_realms(&self) -> usize;

    fn new_object(&mut self, realm: Self::Realm) -> Owned<Self::Value>;
    /// An array of `items` (borrowed) from `realm`'s own `Array`, made
    /// without running script: what a sequence converts to.
    fn new_array(&mut self, realm: Self::Realm, items: &[Self::Value]) -> Owned<Self::Value>;
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
    /// when script is halted ([`Engine::halt`]), with none registered.
    fn call<H: Host<Self>>(
        &mut self,
        host: &mut H,
        f: Self::Value,
        this: Self::Value,
        args: &[Self::Value],
    ) -> Owned<Self::Value>;
    /// A function object that calls `f`, which gets `data` in its
    /// [`crate::Ctx`] on every call: the state of a host function made at
    /// run time (a host microtask's record, a promise reaction). The
    /// engine neither counts nor traces `data`; a host that puts an
    /// address there keeps what it points to alive itself, for as long
    /// as the function may be called.
    fn new_function<H: Host<Self>>(
        &mut self,
        realm: Self::Realm,
        name: &'static str,
        length: u8,
        f: Bound<HostFn, Self, H>,
        data: u64,
    ) -> Owned<Self::Value>;
    /// The interface object of `class` in `realm` (`Node` for the
    /// table of Node): a function whose `prototype` is the prototype
    /// object that the realm's wrappers of `class` inherit from, and
    /// that holds the class's attributes, operations and constants. One
    /// per class and realm, the same object every time; the prototype
    /// objects of a realm chain as the classes do, so that `instanceof`
    /// and a member taken from a prototype work as the web expects.
    /// Calling it throws a TypeError, and so does constructing it while
    /// the class has no constructor; constructing one that has gives the
    /// new object the prototype `new.target` names, so that a subclass
    /// script declares gets its own. The host puts the ones it exposes on the global.
    fn interface_object<H: Host<Self>>(
        &mut self,
        realm: Self::Realm,
        class: &'static ClassDef<Self, H>,
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
    /// After `None` the host never passes that word to the engine again,
    /// and the old wrapper no longer holds the host object (the engine's
    /// graph does not report that edge); the engine frees its memory
    /// itself once its count reaches 0.
    ///
    /// The engine does not reuse a wrapper's memory before its notice is
    /// delivered ([`Engine::drain_host`]) or refused here: a word the
    /// host still holds is safe to pass.
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
    /// hand, and free those wrappers. How many were delivered. The host
    /// calls it whenever it settles (after each call into the isolate
    /// that can release values, and before and after a collection it
    /// drives); until then a dead wrapper's memory and its host object's
    /// count stay held.
    fn drain_host<H: Host<Self>>(&mut self, host: &mut H) -> usize;

    /// Whether the host must say where wrappers are
    /// ([`Engine::place_wrappers`]): true for an engine that frees a
    /// wrapper once script no longer reaches it, whatever counts its
    /// host object has.
    const PLACES_WRAPPERS: bool = false;
    /// The host objects of the wrappers whose words are `words` are in
    /// `tree` now: a node made, inserted or removed. Such an engine keeps
    /// the wrappers of one tree reachable from one another, so that
    /// script holding any one keeps all of them, with what script put on
    /// them, and keeps those of an anchored document as long as the realm.
    ///
    /// A word whose wrapper is gone has its index put in `refused`,
    /// which the host treats as [`Engine::wrapper_upgrade`] returning
    /// `None` for it. `With` names a live wrapper the caller holds.
    /// `false` when the engine could not place them now (script is being
    /// stopped): nothing was placed or refused, and the host asks again
    /// before script next runs.
    ///
    /// A host that holds a node of a detached tree keeps the tree by a
    /// wrapper in it: a host object as a member of its own wrapper
    /// ([`Engine::member`]), an operation by a handle while it runs.
    /// Anchoring the tree would root cycles that run through script.
    fn place_wrappers(&mut self, tree: Tree<Self::Value>, words: &[u64], refused: &mut Vec<usize>) -> bool {
        let _ = (tree, words, refused);
        true
    }

    /// A count on `v` for a [`crate::Member`] of the host object whose
    /// wrapper is `owner` (live, held by the caller). On an engine that
    /// places wrappers, `v` lives exactly as long as that wrapper: it is
    /// kept beside the wrapper in the engine's own heap, so a cycle
    /// through script (a listener whose closure holds its node's
    /// wrapper) is one the engine's collector sees. The host then drops
    /// such members unreleased once it lets go of the wrapper word
    /// (`wrapper_died`, or a refused wrapper).
    fn member(&mut self, owner: Self::Value, v: Self::Value) -> Owned<Self::Value> {
        let _ = owner;
        self.retain(v)
    }

    /// Give back a count [`Engine::member`] gave for the wrapper whose
    /// word is `owner`, while the host still holds that word.
    fn release_member(&mut self, owner: u64, v: Owned<Self::Value>) {
        let _ = owner;
        self.release(v);
    }

    /// A weak handle to `v`. A weak handle to a wrapper does not
    /// preserve it: the wrapper may die while its host object lives, and
    /// the handle then reads as gone.
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
    /// microtask, its record in the function's data word or in `arg`).
    /// The job's realm is the realm of its function, as a promise job's
    /// is its handler's; a job whose realm is detached by the time it
    /// would run is dropped unrun.
    fn enqueue_microtask(&mut self, job: Owned<Self::Value>, arg: Owned<Self::Value>);
    /// Run microtasks until the queue is empty. The host calls it at
    /// each checkpoint: after a call into script returns with
    /// [`Engine::in_script`] false, and at the end of every task; never
    /// while script is on the stack (a listener of an event script
    /// dispatched runs with no checkpoint after it).
    ///
    /// - A checkpoint entered while one runs returns at once.
    /// - Microtasks queued while draining run in the same drain.
    /// - A microtask's exception goes to [`Host::report_exception`] and
    ///   the draining goes on.
    /// - When script is halted during the drain ([`Engine::halt`]) the
    ///   draining stops, and nothing is reported. After a termination the
    ///   queue is gone; after an abort what is left stays queued, and
    ///   nothing runs until [`Engine::resume`].
    fn run_microtasks<H: Host<Self>>(&mut self, host: &mut H);

    /// Whether the host is inside a call into the isolate that can run
    /// script or host functions: false exactly when no such call is on
    /// the stack. Only false against true is promised, so an engine
    /// need not count every script function call: the host entering it
    /// and it entering a host function are enough.
    fn in_script(&self) -> bool;
    /// Whether script may run, as of the last [`crate::Interrupt`]
    /// request the isolate has seen: a request is seen by the time the
    /// isolate is next entered, not when it is sent.
    /// While halted, calls, checkpoints, evaluations and handler
    /// compilations return thrown at once and run nothing, and the host
    /// reports nothing; a host that saw a call end this way skips that
    /// task's checkpoint. No later entry clears it.
    ///
    /// Such an entry leaves no exception of its own: one script
    /// registered while it ran is dropped, and `throw` registers none.
    /// An exception the host registered before the entry is not script's
    /// and stays, for [`Engine::take_exception`].
    fn halt(&self) -> Halt;
    /// Let script run again after an abort; only while not
    /// [`Engine::in_script`]. What [`Engine::halt`] reads after it:
    /// `Running`, or `Terminated` when the isolate was terminated, which
    /// it stays. A termination another thread sends may be seen by this
    /// very call, so the host cannot rule it out and goes by the result,
    /// not by having called it. A no-op when running. `Err` when the
    /// engine has no recoverable abort.
    fn resume(&mut self) -> Result<Halt, Unsupported>;
    /// Drop the queued microtasks without running them: the host's
    /// choice after an abort, before it resumes. Does nothing once
    /// terminated: the termination dropped the queue already. `Err` when
    /// the engine has no recoverable abort.
    fn discard_microtasks(&mut self) -> Result<(), Unsupported>;

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
