//! A worker's global object, plain properties put on before its script
//! runs, as the window's are (`window.rs`): `self`, `name`, the
//! interfaces exposed everywhere, the window's listener methods, timers,
//! `queueMicrotask`, `atob` and `btoa` filed under the worker's id, and
//! its own `postMessage`, `close`, and `onmessage`, `onmessageerror` and
//! `onerror` as accessors. Its `onerror` is called with the ErrorEvent,
//! as other handlers are, not with the five arguments a global's error
//! handler gets in the standard, and its result is read as theirs is.

use madcawl_js::{Bound, ClassDef, Ctx, Engine, HostFn, Owned};
use madcawl_str::{Atom, DomString, atom};

use crate::convert::{arg, event_handler, run};
use crate::event::Target;
use crate::generated as g;
use crate::host::DomHost;
use crate::shell;
use crate::window::put;
use crate::workers::WorkerTask;

impl<E: Engine> DomHost<E> {
    /// The interfaces on a worker's global, in the order of its own keys.
    pub(crate) const WORKER_INTERFACES: &'static [&'static ClassDef<E, DomHost<E>>] = &[
        &g::AbortControllerClass::<E>::DEF,
        &g::AbortSignalClass::<E>::DEF,
        &g::CustomEventClass::<E>::DEF,
        &g::DOMExceptionClass::<E>::DEF,
        &g::EventTargetClass::<E>::DEF,
        &g::EventClass::<E>::DEF,
        &g::ProgressEventClass::<E>::DEF,
        &g::MessageEventClass::<E>::DEF,
        &g::ErrorEventClass::<E>::DEF,
        &g::TextDecoderClass::<E>::DEF,
        &g::TextEncoderClass::<E>::DEF,
        &g::URLClass::<E>::DEF,
        &g::URLSearchParamsClass::<E>::DEF,
        &g::BlobClass::<E>::DEF,
    ];

    /// Put worker `w`'s members on its global, named `name`.
    pub(crate) fn install_worker_global(&mut self, e: &mut E, w: u64, name: &DomString) {
        let realm = self.workers.live[&w].realm;
        let global = e.global(realm);
        let gv = global.get();
        put(e, self, gv, "self", gv);
        let n = e.string(name.clone());
        put(e, self, gv, "name", n.get());
        e.release(n);
        let ok = e.define_interfaces(self, realm, gv, Self::WORKER_INTERFACES);
        assert!(ok, "putting the interface objects on a worker's global");
        self.install_functions(e, realm, gv, w);
        for (fname, length, f) in [("postMessage", 1, post_message_abi::<E> as HostFn), ("close", 0, close_abi::<E>)] {
            let v = self.worker_function(e, realm, fname, length, f, w);
            put(e, self, gv, fname, v.get());
            e.release(v);
        }
        let accessors: [(&str, HostFn, HostFn); 3] = [
            ("onmessage", handler_abi::<E, 0>, set_handler_abi::<E, 0>),
            ("onmessageerror", handler_abi::<E, 1>, set_handler_abi::<E, 1>),
            ("onerror", handler_abi::<E, 2>, set_handler_abi::<E, 2>),
        ];
        for (aname, get, set) in accessors {
            let get = self.worker_function(e, realm, aname, 0, get, w);
            let set = self.worker_function(e, realm, aname, 1, set, w);
            let ok = e.define_accessor(realm, gv, aname, get.get(), set.get(), false);
            assert!(ok, "defining {aname} on a worker's global");
            e.release(get);
            e.release(set);
        }
        e.release(global);
    }

    fn worker_function(&mut self, e: &mut E, realm: E::Realm, name: &'static str, length: u8, f: HostFn, w: u64) -> Owned<E::Value> {
        // SAFETY: every function here reads its context as E and DomHost<E>
        let bound = unsafe { Bound::<HostFn, E, DomHost<E>>::host_fn(f) };
        e.new_function(realm, name, length, bound, w)
    }
}

fn handler_type(i: usize) -> Atom {
    [atom!("message"), atom!("messageerror"), atom!("error")][i]
}

fn post_message<E: Engine>(e: &mut E, h: &mut DomHost<E>, w: u64, _: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    run(e, h, |e, h| {
        crate::convert::need(e, args, 1)?;
        h.post_to_object(e, w, arg::<E>(args, 0))
    })
}

/// The worker closes: nothing more reaches it, its timers go now, and
/// the rest of it once this task is over.
fn close<E: Engine>(e: &mut E, h: &mut DomHost<E>, w: u64, _: E::Value, _: &[E::Value]) -> Owned<E::Value> {
    run(e, h, |e, h| {
        let Some(k) = h.workers.live.get_mut(&w).filter(|k| !k.closing) else { return Ok(()) };
        k.closing = true;
        for t in h.event_loop.remove_window_timers(w) {
            t.handler.release(e);
            t.args.into_iter().for_each(|a| a.release(e));
        }
        h.queue_worker(WorkerTask::Close(w));
        Ok(())
    })
}

fn handler<E: Engine, const I: usize>(e: &mut E, h: &mut DomHost<E>, w: u64, _: E::Value, _: &[E::Value]) -> Owned<E::Value> {
    let ty = handler_type(I);
    h.event_handler(e, Target::Window(w), ty)
}

fn set_handler<E: Engine, const I: usize>(e: &mut E, h: &mut DomHost<E>, w: u64, _: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    run(e, h, |e, h| {
        let ty = handler_type(I);
        if h.workers.live.contains_key(&w) {
            let v = event_handler::<E>(e, arg::<E>(args, 0));
            h.set_event_handler(e, Target::Window(w), ty, v);
        }
        Ok(())
    })
}

// SAFETY: called as a host function made for E and DomHost<E>
unsafe extern "C" fn post_message_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
    unsafe { shell::op_data(ctx, this, argv, argc, post_message::<E>) }
}

// SAFETY: as above
unsafe extern "C" fn close_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
    unsafe { shell::op_data(ctx, this, argv, argc, close::<E>) }
}

// SAFETY: as above
unsafe extern "C" fn handler_abi<E: Engine, const I: usize>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
    unsafe { shell::op_data(ctx, this, argv, argc, handler::<E, I>) }
}

// SAFETY: as above
unsafe extern "C" fn set_handler_abi<E: Engine, const I: usize>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
    unsafe { shell::op_data(ctx, this, argv, argc, set_handler::<E, I>) }
}
