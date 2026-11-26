//! One isolate: a context group, its realms, and what the adapter keeps
//! beside the framework's heap.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use madcawl_js::{Halt, HostRef, Owned};

use crate::ffi::{self, JSContextGroupRef, JSContextRef, JSGlobalContextRef, JSStringRef};
use crate::halt::Flags;
use crate::objects::{Classes, WrapRec};
use crate::protos::RealmData;
use crate::value::Value;

/// A realm's number in its isolate, and the isolate's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Realm(pub(crate) u32, pub(crate) u32);

/// What the framework's finalizers write: they run inside whatever
/// call allocated, so they never touch the isolate itself.
#[derive(Default)]
pub(crate) struct Shared {
    /// Wrappers the collector freed, whose host has not been told.
    pub notices: RefCell<Vec<*mut WrapRec>>,
}

pub(crate) enum WeakSlot {
    /// A `WeakRef` to an object, protected.
    Ref(Value),
    /// A value no collector frees while it is held: an immediate, or a
    /// string held strongly.
    Strong(Value),
}

/// One isolate of the system JavaScriptCore: one context group, whose
/// global contexts are the realms. Values pass between its realms;
/// none passes to another isolate.
pub struct Jsc {
    pub(crate) id: u32,
    pub(crate) group: JSContextGroupRef,
    /// A context of no realm, for what needs a context and no realm:
    /// counts, strings, type checks.
    pub(crate) base: JSGlobalContextRef,
    pub(crate) realms: Vec<RealmData>,
    /// The realm of the last call that named one; errors the adapter
    /// makes come from its constructors.
    pub(crate) current: u32,
    pub(crate) classes: Classes,
    pub(crate) shared: *const Shared,
    /// Registered and not yet taken; holds a count.
    pub(crate) exception: Option<Value>,
    /// Calls into the framework that can run script, on the stack.
    pub(crate) depth: u32,
    pub(crate) halt: Halt,
    pub(crate) flags: Arc<Flags>,
    /// Host microtasks queued while no script ran: queued in the
    /// framework at the next checkpoint, in order.
    pub(crate) pending: Vec<Value>,
    pub(crate) in_checkpoint: bool,
    /// The spelling of each static atom, made once.
    pub(crate) atom_strings: HashMap<u32, JSStringRef>,
    pub(crate) weak: Vec<Option<WeakSlot>>,
    pub(crate) weak_free: Vec<u32>,
    pub(crate) suspects: Vec<HostRef>,
    pub(crate) suspected: HashSet<HostRef>,
    pub(crate) limits: (usize, usize),
    pub(crate) external: isize,
    pub(crate) pressure_told: bool,
    /// UTF-16 scratch for strings going in.
    pub(crate) scratch: Vec<u16>,
    /// Each WindowProxy's word: the record whose `g` is its target,
    /// counted, and the realm it forwards to.
    pub(crate) proxies: HashMap<u64, (Value, Realm)>,
    /// The first realm's `deref` helper, counted: weak handles read
    /// through it once no realm is live.
    pub(crate) deref: Value,
    /// The private property name a wrapper's members hang on.
    pub(crate) members_name: JSStringRef,
    /// How many times a member was taken out of a container.
    pub(crate) member_writes: u64,
}

impl Jsc {
    /// A new isolate with the watchdog that lets another thread stop
    /// running script ([`madcawl_js::Interrupt`]).
    pub fn new() -> Self {
        Self::build(true)
    }

    /// An isolate whose script cannot be interrupted while it runs: an
    /// interrupt is seen only at the next entry. For measuring what the
    /// watchdog costs.
    pub fn without_watchdog() -> Self {
        Self::build(false)
    }

    fn build(watchdog: bool) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        // SAFETY: plain constructors; the group is released in Drop
        let (group, base) = unsafe {
            let group = ffi::JSContextGroupCreate();
            (group, ffi::JSGlobalContextCreateInGroup(group, std::ptr::null_mut()))
        };
        crate::value::check_layout(base);
        let flags = Arc::new(Flags::default());
        if watchdog {
            crate::private::install_watchdog(group, Arc::as_ptr(&flags));
        }
        Jsc {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            group,
            base,
            realms: Vec::new(),
            current: 0,
            classes: Classes::new(),
            shared: Box::into_raw(Box::default()),
            exception: None,
            depth: 0,
            halt: Halt::Running,
            flags,
            pending: Vec::new(),
            in_checkpoint: false,
            atom_strings: HashMap::new(),
            weak: Vec::new(),
            weak_free: Vec::new(),
            suspects: Vec::new(),
            suspected: HashSet::new(),
            limits: (usize::MAX, usize::MAX),
            external: 0,
            pressure_told: false,
            scratch: Vec::new(),
            proxies: HashMap::new(),
            deref: Value(0),
            // SAFETY: a static string; released in Drop
            members_name: unsafe { ffi::JSStringCreateWithUTF8CString(c"members".as_ptr()) },
            member_writes: 0,
        }
    }

    /// The context calls without a realm of their own go through.
    pub(crate) fn ctx(&self) -> JSContextRef {
        match self.realms.get(self.current as usize) {
            Some(r) if !r.detached => r.ctx,
            _ => self.base,
        }
    }

    /// The current realm when it is live, or else the first live one.
    pub(crate) fn live_realm(&self) -> Option<u32> {
        let live = |i: u32| self.realms.get(i as usize).is_some_and(|r| !r.detached);
        if live(self.current) {
            return Some(self.current);
        }
        (0..self.realms.len() as u32).find(|&i| live(i))
    }

    pub(crate) fn realm_ctx(&mut self, r: Realm) -> JSContextRef {
        let i = self.index(r);
        self.current = i;
        let d = &self.realms[i as usize];
        assert!(!d.detached, "a detached realm");
        d.ctx
    }

    pub(crate) fn index(&self, r: Realm) -> u32 {
        assert_eq!(r.1, self.id, "a realm of another isolate");
        r.0
    }

    pub(crate) fn owned(v: Value) -> Owned<Value> {
        // SAFETY: called on a value whose count the caller passes on
        unsafe { Owned::from_raw(v) }
    }

    /// One more count on `v`: the framework's protection, for a cell.
    pub(crate) fn protect(&self, v: Value) {
        if v.is_cell() {
            // SAFETY: a live value of this isolate
            unsafe { ffi::JSValueProtect(self.base, v.raw()) };
        }
    }

    pub(crate) fn unprotect(&self, v: Value) {
        if v.is_cell() {
            // SAFETY: a value this isolate protected
            unsafe { ffi::JSValueUnprotect(self.base, v.raw()) };
        }
    }

    /// `v`, just returned by the framework, with a count for the caller.
    pub(crate) fn take(&self, v: Value) -> Owned<Value> {
        self.protect(v);
        Jsc::owned(v)
    }

    pub(crate) fn halted(&self) -> bool {
        self.halt != Halt::Running
    }

    pub(crate) fn shared(&self) -> &Shared {
        // SAFETY: freed only in Drop
        unsafe { &*self.shared }
    }
}

impl Default for Jsc {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Jsc {
    fn drop(&mut self) {
        if let Some(e) = self.exception.take() {
            self.unprotect(e);
        }
        self.drop_pending();
        self.unprotect(self.deref);
        for (p, (f, _)) in std::mem::take(&mut self.proxies) {
            self.unprotect(Value(p));
            self.unprotect(f);
        }
        for slot in std::mem::take(&mut self.weak).into_iter().flatten() {
            let (WeakSlot::Ref(v) | WeakSlot::Strong(v)) = slot;
            self.unprotect(v);
        }
        for r in 0..self.realms.len() as u32 {
            crate::protos::detach(self, r);
        }
        for (_, s) in self.atom_strings.drain() {
            // SAFETY: made by this isolate, used by nothing now
            unsafe { ffi::JSStringRelease(s) };
        }
        // SAFETY: made in build, used by nothing now
        unsafe { ffi::JSStringRelease(self.members_name) };
        // SAFETY: every realm's context was released by detach; the group
        // goes last, and its heap with it, running the finalizers
        unsafe {
            ffi::JSGlobalContextRelease(self.base);
            ffi::JSContextGroupRelease(self.group);
        }
        self.classes.release();
        // the finalizers that ran queued their records here
        // SAFETY: allocated in build, and the heap that wrote it is gone
        let shared = unsafe { Box::from_raw(self.shared.cast_mut()) };
        for rec in shared.notices.take() {
            // SAFETY: a record whose wrapper was finalized
            drop(unsafe { Box::from_raw(rec) });
        }
    }
}
