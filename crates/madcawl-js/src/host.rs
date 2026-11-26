//! The embedder's side of the engine interface.

use crate::class::ClassDef;
use crate::engine::Engine;
use crate::script::ModuleRequest;
use crate::value::{HostRef, Owned};

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

    /// An [`crate::Interrupt::request_callback`] request, seen on the
    /// isolate's thread; script may be on the stack. It may read and
    /// change host state and make values, and runs no script.
    fn interrupt_callback(&mut self, e: &mut E) {
        let _ = e;
    }

    /// Memory use went past the soft limit
    /// ([`Control::set_memory_limits`]): the host frees what it can
    /// (caches, a collection soon). Called once each time use crosses
    /// the limit going up, on the isolate's thread; it runs no script.
    fn memory_pressure(&mut self, e: &mut E) {
        let _ = e;
    }

    /// The host's monotonic clock in nanoseconds, which
    /// [`crate::Deadline::At`] is read against: the engine reads no
    /// clock of its own. The default counts from the first call in the
    /// process.
    fn now(&self) -> u64 {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_nanos() as u64
    }
}
