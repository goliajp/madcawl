//! What an engine does for the windows of browsing contexts: the
//! WindowProxy, the object that stands for a window of another origin,
//! and wrappers moving from one window's realm to another's.

use crate::engine::Engine;
use crate::value::Owned;

/// The methods take the engine's own types, so they are bound by
/// `Engine`, as those of [`crate::Builtins`] are.
pub trait Windows {
    /// A WindowProxy: the object script gets for a browsing context's
    /// window, forwarding every property access to the global object of
    /// `realm`; thrown, with nothing registered, when script is halted
    /// and the engine needs script to make one. What `this` the global's
    /// own accessors see is settled with the global as a host object.
    fn new_window_proxy(&mut self, realm: <Self as Engine>::Realm) -> Owned<<Self as Engine>::Value>
    where
        Self: Engine;

    /// Navigation: `proxy` forwards to the global of `realm` from now on.
    /// Script that holds the proxy sees the new global through the same
    /// object; the old realm is detached by the host as usual.
    ///
    /// `realm` is one of this isolate's. A window in another agent
    /// cluster (a cross-site iframe, an opener) is never a target: the
    /// host gives script an object that stands for it instead.
    fn set_window_proxy_target(&mut self, proxy: <Self as Engine>::Value, realm: <Self as Engine>::Realm)
    where
        Self: Engine;

    /// The realm `proxy` forwards to; `None` when `proxy` is no
    /// WindowProxy, which it may no longer be once that realm is detached.
    fn window_proxy_target(&self, proxy: <Self as Engine>::Value) -> Option<<Self as Engine>::Realm>
    where
        Self: Engine;

    /// What script gets for a window of another origin: an object of
    /// `realm` with no prototype, through which the own properties of
    /// `members` read (it and its functions frozen). `then`,
    /// @@toStringTag, @@hasInstance and @@isConcatSpreadable, the
    /// standard's cross-origin fallback, are its own keys too, each
    /// configurable and undefined; any other read, write, definition,
    /// deletion or `in` calls `deny` with the key, which throws. Thrown,
    /// with nothing made, when script is halted and the engine needs
    /// script to make it.
    fn new_cross_origin_object(
        &mut self,
        realm: <Self as Engine>::Realm,
        members: <Self as Engine>::Value,
        deny: <Self as Engine>::Value,
    ) -> Owned<<Self as Engine>::Value>
    where
        Self: Engine;

    /// The host object of the live wrapper whose word is `word` moved to
    /// a document of `realm`'s window: detaching the realm it was made
    /// in leaves it alone from now on, as one made in `realm`; its
    /// prototype and what script put on it stay. Only an engine whose
    /// wrappers end with their realm does anything.
    fn move_wrapper(&mut self, word: u64, realm: <Self as Engine>::Realm)
    where
        Self: Engine,
    {
        let _ = (word, realm);
    }
}
