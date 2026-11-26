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
        if self.halt == madcawl_js::Halt::Terminated {
            self.unprotect(job.into_raw());
            self.unprotect(arg.into_raw());
            return;
        }
        let realm = self.current;
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
        let r = self.call_helper(realm, Helper::EnqueueAll, &[arr]);
        assert!(r.is_ok(), "queueing a microtask");
        self.unprotect(arr);
        self.unprotect(t);
    }

    /// A microtask checkpoint, called by the host with no script on the
    /// stack: queue the host microtasks waiting, and the framework runs
    /// them as this call returns.
    pub(crate) fn checkpoint<H: Host<Jsc>>(&mut self, host: &mut H) {
        let me: *mut Jsc = self;
        let _frame = crate::frame::enter(me, host);
        self.poll(host);
        if self.in_checkpoint || self.halted() || self.pending.is_empty() {
            return;
        }
        self.in_checkpoint = true;
        let jobs = std::mem::take(&mut self.pending);
        let realm = self.current;
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

    /// An array of `items`, counted.
    pub(crate) fn array(&self, ctx: JSContextRef, items: &[Value]) -> Value {
        // SAFETY: live values; `Value` is the word
        let a = Value(unsafe { ffi::JSObjectMakeArray(ctx, items.len(), items.as_ptr().cast(), std::ptr::null_mut()) } as u64);
        self.protect(a);
        a
    }
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
