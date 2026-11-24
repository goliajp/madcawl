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

use crate::engine::{Engine, Tracer, Unlinker};
use crate::value::{NotSend, Owned};

pub struct Member<E: Engine>(Option<E::Value>, NotSend);

impl<E: Engine> Member<E> {
    pub fn new(v: Owned<E::Value>) -> Self {
        Member(Some(v.into_raw()), PhantomData)
    }

    pub fn empty() -> Self {
        Member(None, PhantomData)
    }

    pub fn get(&self) -> Option<E::Value> {
        self.0
    }

    pub fn trace(&self, t: &mut E::Tracer) {
        if let Some(v) = self.0 {
            t.value(v);
        }
    }

    pub fn unlink(&mut self, u: &mut E::Unlinker) {
        if let Some(v) = self.0.take() {
            // SAFETY: the count this handle held
            u.release(unsafe { Owned::from_raw(v) });
        }
    }

    pub fn clear(&mut self, e: &mut E) {
        if let Some(v) = self.0.take() {
            // SAFETY: as in unlink
            e.release(unsafe { Owned::from_raw(v) });
        }
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
