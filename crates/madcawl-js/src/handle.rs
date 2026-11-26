//! Handles the host holds script values through.
//!
//! - [`Member`]: owned by a host object, which reports it from
//!   `traverse` and gives it up in `unlink`: listener lists, script
//!   values cached on a node.
//! - [`Root`]: held by no host object and never reported, so its target
//!   lives while it is held: a timer's callback, a fetch in flight, an
//!   event being dispatched.
//! - Weak: [`crate::Engine::weak`]; a node's wrapper word is the weak
//!   handle the engine itself clears through `wrapper_died`, and the
//!   host reads through [`crate::Engine::wrapper_upgrade`].
//!
//! None of them gives its count back on drop, since that needs the
//! engine: each has a method that takes it. None of them is `Send`: a
//! count belongs to the isolate's thread.

use std::marker::PhantomData;

use crate::engine::Engine;
use crate::graph::{Tracer, Unlinker};
use crate::value::{NotSend, Owned};

pub struct Member<E: Engine> {
    v: Option<E::Value>,
    /// The owner's wrapper word when the count came from
    /// [`Engine::member`]; 0 for one from a plain count.
    owner: u64,
    _not_send: NotSend,
}

impl<E: Engine> Member<E> {
    pub fn new(v: Owned<E::Value>) -> Self {
        Member { v: Some(v.into_raw()), owner: 0, _not_send: PhantomData }
    }

    /// A member of the host object whose wrapper is `owner` (live, held
    /// by the caller): see [`Engine::member`].
    pub fn of(e: &mut E, owner: E::Value, v: E::Value) -> Self {
        let m = e.member(owner, v);
        Member { v: Some(m.into_raw()), owner: E::wrapper_word(owner), _not_send: PhantomData }
    }

    pub fn empty() -> Self {
        Member { v: None, owner: 0, _not_send: PhantomData }
    }

    pub fn get(&self) -> Option<E::Value> {
        self.v
    }

    pub fn trace(&self, t: &mut E::Tracer) {
        if let Some(v) = self.v {
            t.value(v);
        }
    }

    pub fn unlink(&mut self, u: &mut E::Unlinker) {
        if let Some(v) = self.v.take() {
            // SAFETY: the count this handle held
            u.release(unsafe { Owned::from_raw(v) });
        }
    }

    pub fn clear(&mut self, e: &mut E) {
        if let Some(v) = self.v.take() {
            // SAFETY: as in unlink
            let v = unsafe { Owned::from_raw(v) };
            if self.owner == 0 { e.release(v) } else { e.release_member(self.owner, v) }
        }
    }

    /// Let go of the value without the engine: its owner's wrapper died
    /// on an engine that keeps members beside wrappers, and the value
    /// with it ([`Engine::member`]).
    pub fn forget(&mut self) {
        self.v = None;
    }
}

pub struct Root<E: Engine>(E::Value, NotSend);

impl<E: Engine> Root<E> {
    pub fn new(v: Owned<E::Value>) -> Self {
        Root(v.into_raw(), PhantomData)
    }

    pub fn get(&self) -> E::Value {
        self.0
    }

    pub fn release(self, e: &mut E) {
        // SAFETY: the count this handle held
        e.release(unsafe { Owned::from_raw(self.0) });
    }
}
