//! Microtasks.
//!
//! The framework has one queue, which it drains itself when the
//! outermost call into it returns, and no checkpoint of its own. That
//! moment is the checkpoint "a call into script returned with no script
//! on the stack", so promise jobs need nothing from the adapter.
//!
//! A host microtask goes into the same queue, as a reaction to an
//! already resolved promise, so it runs in order with the promise jobs.
//! Its function is a trampoline that calls `job(arg)` and hands an
//! exception to the host. Queued while script runs, it goes in at once
//! and runs when the outermost call returns. Queued while no script
//! runs, it waits in the adapter: queueing it then would itself be an
//! outermost call and run it at once. [`Jsc::checkpoint`] queues the
//! waiting ones in one call, which runs them, and whatever they queue,
//! before it returns.

use madcawl_js::{Host, Owned};

use crate::ffi::{self, JSContextRef, JSValueRef};
use crate::isolate::Jsc;
use crate::objects::{FnKind, FnRec, Report, make_function};
use crate::protos::Helper;
use crate::value::Value;

impl Jsc {
    pub(crate) fn queue_job(&mut self, job: Owned<Value>, arg: Owned<Value>) {
        let realm = self.current;
        let detached = self.realms.get(realm as usize).is_none_or(|r| r.detached);
        if self.halt == madcawl_js::Halt::Terminated || detached {
            self.unprotect(job.into_raw());
            self.unprotect(arg.into_raw());
            return;
        }
        let kind = FnKind::Micro { job: job.into_raw(), arg: arg.into_raw(), realm };
        let ctx = self.realms[realm as usize].ctx;
        let fproto = self.helper(realm, Helper::FunctionProto);
        let rec = FnRec { kind, host_type: std::any::TypeId::of::<()>() };
        let t = make_function(self, ctx, rec, fproto);
        if self.depth == 0 {
            self.pending.push(t);
            return;
        }
        let arr = self.array(ctx, &[t]);
        if self.call_helper(realm, Helper::EnqueueAll, &[arr]).is_err() {
            // script is being stopped (or out of stack): the job goes;
            // a trampoline the framework queued after all runs nothing
            self.after_stop();
            self.release_trampoline(t);
        } else {
            self.unprotect(t);
        }
        self.unprotect(arr);
    }

    /// A microtask checkpoint, called by the host with no script on the
    /// stack: queue the host microtasks waiting, and the framework runs
    /// them as this call returns.
    ///
    /// Host microtasks queued with no script running wait here, while a
    /// promise job queued by the next call into script goes into the
    /// framework's queue and runs as that call returns, ahead of them.
    /// Nothing in the bindings queues a host microtask outside script
    /// yet; one that does must reach a checkpoint before calling script.
    pub(crate) fn checkpoint<H: Host<Jsc>>(&mut self, host: &mut H) {
        let me: *mut Jsc = self;
        let _frame = crate::frame::enter(me, host);
        self.poll(host);
        if self.in_checkpoint || self.halted() || self.pending.is_empty() {
            return;
        }
        let Some(realm) = self.live_realm() else {
            self.drop_pending();
            return;
        };
        self.in_checkpoint = true;
        let jobs = std::mem::take(&mut self.pending);
        let ctx = self.realms[realm as usize].ctx;
        let arr = self.array(ctx, &jobs);
        for t in jobs {
            self.unprotect(t);
        }
        if self.call_helper(realm, Helper::EnqueueAll, &[arr]).is_err() {
            self.after_stop();
        }
        self.unprotect(arr);
        self.in_checkpoint = false;
    }

    /// Let go of the waiting host microtasks unrun, with what each holds.
    pub(crate) fn drop_pending(&mut self) {
        for t in std::mem::take(&mut self.pending) {
            self.release_trampoline(t);
        }
    }

    /// Let go of the waiting host microtasks of realm `r`, being detached.
    pub(crate) fn drop_pending_of(&mut self, r: u32) {
        for t in std::mem::take(&mut self.pending) {
            if trampoline(t).map(|k| matches!(k, FnKind::Micro { realm, .. } if *realm == r)) == Some(true) {
                self.release_trampoline(t);
            } else {
                self.pending.push(t);
            }
        }
    }

    /// Drop trampoline `t`'s count, and its job and argument unless it
    /// ran: a finalizer cannot release them.
    pub(crate) fn release_trampoline(&self, t: Value) {
        if let Some(kind) = trampoline(t)
            && let FnKind::Micro { job, arg, .. } = std::mem::replace(kind, FnKind::Ran)
        {
            self.unprotect(job);
            self.unprotect(arg);
        }
        self.unprotect(t);
    }

    /// An array of `items`, counted.
    pub(crate) fn array(&self, ctx: JSContextRef, items: &[Value]) -> Value {
        // SAFETY: live values; `Value` is the word
        let a = Value(unsafe { ffi::JSObjectMakeArray(ctx, items.len(), items.as_ptr().cast(), std::ptr::null_mut()) } as u64);
        self.protect(a);
        a
    }
}

/// The record of trampoline `t`, a counted function object of the
/// adapter's function class.
fn trampoline<'a>(t: Value) -> Option<&'a mut FnKind> {
    // SAFETY: a live function object, whose private data is its record
    let rec = unsafe { ffi::JSObjectGetPrivate(t.0 as _).cast::<FnRec>().as_mut() }?;
    Some(&mut rec.kind)
}

/// Run the job of a trampoline, once.
pub(crate) fn run_job(e: &mut Jsc, ctx: JSContextRef, rec: &mut FnRec, host: *mut (), report: Report) {
    let FnKind::Micro { job, arg, realm } = std::mem::replace(&mut rec.kind, FnKind::Ran) else {
        return;
    };
    let detached = e.realms[realm as usize].detached;
    if !detached && !e.halted() {
        let mut exc: JSValueRef = std::ptr::null();
        // SAFETY: a counted job and argument of this isolate
        unsafe { ffi::JSObjectCallAsFunction(ctx, job.0 as _, std::ptr::null_mut(), 1, &arg.raw(), &mut exc) };
        if !exc.is_null() {
            e.after_stop();
            if !e.halted() {
                let x = Value::from_ref(exc);
                e.protect(x);
                // SAFETY: the frame's engine and host, of the type the job was queued for
                unsafe { report(e, host, x) };
            }
        }
    }
    e.unprotect(job);
    e.unprotect(arg);
}
