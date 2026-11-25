//! The stub isolate: its cells, counts and realms.

use std::collections::{HashMap, HashSet};
use std::mem::ManuallyDrop;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use madcawl_js::{Halt, Key, Owned};
use madcawl_str::{Atom, DomString, HeldAtoms};

use crate::cell::{Body, Cell, PropKey};
use crate::collect::{Collector, Vertex};
use crate::value::Value;

/// A realm's number in its isolate, and the isolate's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Realm(pub(crate) u32, pub(crate) u32);

pub(crate) struct RealmData {
    pub global: Value,
    pub detached: bool,
    /// The function objects of host operations, one per class table
    /// entry (class id, entry), so that reading a method twice gives the
    /// same function. Class tables are keyed by id, not address: a table
    /// that is a constant may have several addresses.
    pub methods: HashMap<(u16, u16), Value>,
    /// Each host class's interface object and prototype object, by
    /// class id, made when first asked for.
    pub protos: HashMap<u16, (Value, Value)>,
}

/// One isolate. It runs no script: tests do through the engine
/// interface what script would.
pub struct Stub {
    /// This isolate's number among those of the process.
    pub(crate) id: u32,
    pub(crate) realms: Vec<RealmData>,
    pub(crate) current: u32,
    pub(crate) exception: Option<Value>,
    pub(crate) live: usize,
    pub(crate) names: HeldAtoms,
    pub(crate) microtasks: std::collections::VecDeque<(Value, Value)>,
    pub(crate) in_checkpoint: bool,
    /// Script and host functions on the stack.
    pub(crate) depth: u32,
    /// Set from an interrupt request; only `resume` clears an abort.
    pub(crate) halt: Halt,
    /// Dead wrappers whose host has not been told.
    pub(crate) notices: Vec<Value>,
    /// While a batch of unlinks runs, released values wait here.
    pub(crate) deferred: Option<Vec<Value>>,
    /// Cycle candidates, in the order they came, and the set that says
    /// which entries still stand (the engine's buffered mark).
    pub(crate) buffer: Vec<Vertex>,
    pub(crate) buffered: HashSet<Vertex>,
    pub(crate) collector: Collector,
    pub(crate) weak: Vec<Option<Value>>,
    pub(crate) weak_free: Vec<u32>,
    pub(crate) weak_of: HashMap<Value, Vec<u32>>,
    pub(crate) interrupt: Arc<AtomicU8>,
    /// A callback request seen and not yet delivered to the host.
    pub(crate) callback_due: bool,
    /// Use is past the soft limit and the host was told.
    pub(crate) pressure_told: bool,
    pub(crate) external: isize,
    pub(crate) limits: (usize, usize),
}

impl Stub {
    pub fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        Stub {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            realms: Vec::new(),
            current: 0,
            exception: None,
            live: 0,
            names: HeldAtoms::new(),
            microtasks: Default::default(),
            in_checkpoint: false,
            depth: 0,
            halt: Halt::Running,
            notices: Vec::new(),
            deferred: None,
            buffer: Vec::new(),
            buffered: HashSet::new(),
            collector: Collector::default(),
            weak: Vec::new(),
            weak_free: Vec::new(),
            weak_of: HashMap::new(),
            interrupt: Arc::new(AtomicU8::new(0)),
            callback_due: false,
            pressure_told: false,
            external: 0,
            limits: (usize::MAX, usize::MAX),
        }
    }

    /// Cells alive, wrappers waiting for their host's notice included.
    pub fn live_cells(&self) -> usize {
        self.live
    }

    /// Microtasks waiting in the queue, for tests.
    pub fn queued_microtasks(&self) -> usize {
        self.microtasks.len()
    }

    /// The count of a cell or a string, for tests.
    pub fn count_of(&self, v: Value) -> u32 {
        assert!(v.is_block(), "a counted value");
        // SAFETY: a block value points at a live block, header first
        madcawl_str::abi::refcount(unsafe { *(v.0 as *const u64) })
    }

    pub(crate) fn alloc(&mut self, realm: u32, body: Body) -> Value {
        self.live += 1;
        Cell::alloc(self.id, realm, body)
    }

    /// `c` is this isolate's.
    pub(crate) fn own(&self, c: &Cell) {
        assert_eq!(c.isolate, self.id, "a value of another isolate");
    }

    pub(crate) fn owned(v: Value) -> Owned<Value> {
        // SAFETY: called on a value whose count the caller passes on
        unsafe { Owned::from_raw(v) }
    }

    pub(crate) fn retain_value(&mut self, v: Value) {
        if v.is_string() {
            // SAFETY: a string value is a block pointer with a count
            let s = ManuallyDrop::new(unsafe { string_at(v) });
            std::mem::forget(ManuallyDrop::into_inner(s.clone()));
        } else if v.is_cell() {
            // SAFETY: a live cell
            let c = unsafe { Cell::at(v) };
            self.own(c);
            // as in the engine: a count of 0 is freed memory
            assert_ne!(c.count(), 0, "a count added to a dead object");
            c.set_count(c.count() + 1);
        }
    }

    /// Entering a script or host function; false when script is
    /// halted, and the function is not run.
    pub(crate) fn enter(&mut self) -> bool {
        self.poll_interrupt();
        if self.halted() {
            return false;
        }
        self.depth += 1;
        true
    }

    /// Leaving the function entered; its result, or thrown when script
    /// was halted meanwhile, with no exception left registered.
    pub(crate) fn leave(&mut self, r: Value) -> Value {
        self.poll_interrupt();
        self.depth -= 1;
        if self.halted() {
            self.release_value(r);
            if let Some(e) = self.exception.take() {
                self.release_value(e);
            }
            return crate::value::THROWN;
        }
        r
    }

    /// The index of `r`, which must be this isolate's.
    pub(crate) fn index(&self, r: Realm) -> u32 {
        assert_eq!(r.1, self.id, "a realm of another isolate");
        r.0
    }

    /// The global of a realm that is not detached: detaching gave up its
    /// count.
    pub(crate) fn live_global(&mut self, r: u32) -> Value {
        let d = self.realm_data(r);
        assert!(!d.detached, "a detached realm");
        d.global
    }

    /// Give up one count on `v`, and everything that frees.
    pub(crate) fn release_value(&mut self, v: Value) {
        if let Some(d) = self.deferred.as_mut() {
            if v.is_block() {
                d.push(v);
            }
            return;
        }
        let mut work = vec![v];
        while let Some(v) = work.pop() {
            if v.is_string() {
                // SAFETY: the count being given up
                drop(unsafe { string_at(v) });
                continue;
            }
            if !v.is_cell() {
                continue;
            }
            // SAFETY: a live cell
            let c = unsafe { Cell::at(v) };
            self.own(c);
            let n = c.count() - 1;
            c.set_count(n);
            if n != 0 {
                self.buffer_vertex(Vertex::Cell(v));
                continue;
            }
            c.take_held(&mut work);
            self.clear_weak(v);
            match &mut c.body {
                Body::Wrapper { dead, preserved, .. } => {
                    // kept until the notice is delivered: the host may still
                    // read the wrapper word, and wrapper_upgrade says it died.
                    // The host gave up the preserved edge, in unlink
                    *preserved = false;
                    *dead = true;
                    self.notices.push(v);
                }
                _ => self.free_cell(v),
            }
        }
    }

    pub(crate) fn free_cell(&mut self, v: Value) {
        self.unbuffer(Vertex::Cell(v));
        self.clear_weak(v);
        // SAFETY: a live cell
        match &unsafe { Cell::at(v) }.body {
            Body::WeakRef { slot } => self.weak_slot_drop(*slot),
            Body::WeakMap { entries } => {
                for s in entries.iter().map(|&(s, _)| s).collect::<Vec<_>>() {
                    self.weak_slot_drop(s);
                }
            }
            _ => {}
        }
        self.live -= 1;
        // SAFETY: a cell whose count is 0 and that nothing refers to
        unsafe { Cell::free(v) };
    }

    pub(crate) fn buffer_vertex(&mut self, v: Vertex) {
        if self.buffered.insert(v) {
            self.buffer.push(v);
        }
    }

    pub(crate) fn unbuffer(&mut self, v: Vertex) {
        self.buffered.remove(&v);
        self.collector.forget(v);
    }

    pub(crate) fn clear_weak(&mut self, v: Value) {
        if let Some(slots) = self.weak_of.remove(&v) {
            for s in slots {
                self.weak[s as usize] = None;
            }
        }
    }

    pub(crate) fn key(&self, key: Key<'_>) -> PropKey {
        match key {
            Key::Name(a) => {
                self.names.keep(a);
                PropKey::Name(a)
            }
            Key::Index(i) => PropKey::Index(i),
            Key::Str(s) => {
                let mut b = Vec::new();
                s.write_wtf8(&mut b);
                match std::str::from_utf8(&b).ok().and_then(|t| t.parse::<u32>().ok()) {
                    Some(i) if i != u32::MAX && !(b.len() > 1 && b[0] == b'0') => PropKey::Index(i),
                    _ => PropKey::Name(self.names.hold(&b)),
                }
            }
        }
    }

    pub(crate) fn realm_data(&mut self, r: u32) -> &mut RealmData {
        &mut self.realms[r as usize]
    }
}

impl Default for Stub {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        for r in 0..self.realms.len() as u32 {
            crate::props::detach(self, Realm(r, self.id));
        }
        if let Some(e) = self.exception.take() {
            self.release_value(e);
        }
        self.drop_microtasks();
    }
}

/// # Safety
/// `v` is a string value, and the string made takes one of its counts.
pub(crate) unsafe fn string_at(v: Value) -> DomString {
    // SAFETY: the caller's
    unsafe { DomString::from_raw(NonNull::new_unchecked(v.0 as *mut u8)) }
}

/// The name an atom spells, as bytes.
pub(crate) fn name_bytes(a: Atom) -> Vec<u8> {
    let mut b = Vec::new();
    a.write_wtf8(&mut b);
    b
}

