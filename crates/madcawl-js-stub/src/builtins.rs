//! What script does through builtins the stub has no script for: weak
//! references and maps, freezing, symbols. Each is the operation a test
//! calls where a page would call the builtin; the ones that make a
//! wrapper observable preserve it, as the engine interface lists.

use madcawl_js::{Host, Owned};

use crate::cell::{Body, Cell};
use crate::isolate::{Realm, Stub};
use crate::value::{UNDEFINED, Value};

impl Stub {
    /// A weak handle slot on cell `v`.
    pub(crate) fn weak_slot(&mut self, v: Value) -> u32 {
        assert!(v.is_cell(), "a weak handle to an object");
        let slot = match self.weak_free.pop() {
            Some(s) => {
                self.weak[s as usize] = Some(v);
                s
            }
            None => {
                self.weak.push(Some(v));
                (self.weak.len() - 1) as u32
            }
        };
        self.weak_of.entry(v).or_default().push(slot);
        slot
    }

    pub(crate) fn weak_slot_drop(&mut self, w: u32) {
        if let Some(v) = self.weak[w as usize].take()
            && let Some(slots) = self.weak_of.get_mut(&v)
        {
            slots.retain(|&s| s != w);
        }
        self.weak_free.push(w);
    }

    /// `new WeakRef(target)`.
    pub fn new_weak_ref<H: Host<Stub>>(&mut self, host: &mut H, realm: Realm, target: Value) -> Owned<Value> {
        let slot = self.weak_slot(target);
        self.preserve(host, target);
        Stub::owned(self.alloc(self.index(realm), Body::WeakRef { slot }))
    }

    /// `weakRef.deref()`: the target, or undefined once it is gone.
    pub fn weak_ref_deref(&mut self, weak_ref: Value) -> Owned<Value> {
        // SAFETY: a live cell, read only
        let Body::WeakRef { slot } = unsafe { Cell::at(weak_ref) }.body else { panic!("a WeakRef") };
        let v = self.weak[slot as usize].unwrap_or(UNDEFINED);
        self.retain_value(v);
        Stub::owned(v)
    }

    /// `new WeakMap()`.
    pub fn new_weak_map(&mut self, realm: Realm) -> Owned<Value> {
        Stub::owned(self.alloc(self.index(realm), Body::WeakMap { entries: Vec::new() }))
    }

    /// `map.set(key, value)`; `key` is an object.
    pub fn weak_map_set<H: Host<Stub>>(&mut self, host: &mut H, map: Value, key: Value, value: Value) {
        self.weak_map_prune(map);
        self.retain_value(value);
        let found = self.weak_map_find(map, key);
        let slot = if found.is_none() { Some(self.weak_slot(key)) } else { None };
        // SAFETY: a live cell
        let Body::WeakMap { entries } = &mut unsafe { Cell::at(map) }.body else { panic!("a WeakMap") };
        let old = match (found, slot) {
            (Some(i), _) => Some(std::mem::replace(&mut entries[i].1, value)),
            (None, Some(s)) => {
                entries.push((s, value));
                None
            }
            (None, None) => unreachable!(),
        };
        self.preserve(host, key);
        if let Some(old) = old {
            self.release_value(old);
        }
    }

    /// `map.get(key)`.
    pub fn weak_map_get(&mut self, map: Value, key: Value) -> Owned<Value> {
        self.weak_map_prune(map);
        let v = match self.weak_map_find(map, key) {
            // SAFETY: a live cell, read only
            Some(i) => match &unsafe { Cell::at(map) }.body {
                Body::WeakMap { entries } => entries[i].1,
                _ => unreachable!(),
            },
            None => UNDEFINED,
        };
        self.retain_value(v);
        Stub::owned(v)
    }

    fn weak_map_find(&self, map: Value, key: Value) -> Option<usize> {
        // SAFETY: a live cell, read only
        let Body::WeakMap { entries } = &unsafe { Cell::at(map) }.body else { panic!("a WeakMap") };
        entries.iter().position(|&(s, _)| self.weak[s as usize] == Some(key))
    }

    /// Drop the entries whose key died.
    fn weak_map_prune(&mut self, map: Value) {
        // SAFETY: a live cell
        let Body::WeakMap { entries } = &mut unsafe { Cell::at(map) }.body else { panic!("a WeakMap") };
        let weak = &self.weak;
        let (dead, live): (Vec<_>, Vec<_>) = entries.drain(..).partition(|&(s, _)| weak[s as usize].is_none());
        *entries = live;
        for (s, v) in dead {
            self.weak_slot_drop(s);
            self.release_value(v);
        }
    }

    /// `Object.preventExtensions(obj)`.
    pub fn prevent_extensions<H: Host<Stub>>(&mut self, host: &mut H, obj: Value) {
        // SAFETY: a live cell
        unsafe { Cell::at(obj) }.extensible = false;
        self.preserve(host, obj);
    }

    /// `Object.freeze(obj)`: no property added, changed or deleted.
    pub fn freeze<H: Host<Stub>>(&mut self, host: &mut H, obj: Value) {
        // SAFETY: a live cell
        let c = unsafe { Cell::at(obj) };
        (c.extensible, c.frozen) = (false, true);
        self.preserve(host, obj);
    }

    pub fn is_frozen(&self, obj: Value) -> bool {
        // SAFETY: a live cell, read only
        unsafe { Cell::at(obj) }.frozen
    }

    pub fn is_extensible(&self, obj: Value) -> bool {
        // SAFETY: a live cell, read only
        unsafe { Cell::at(obj) }.extensible
    }

    /// `Symbol()`.
    pub fn new_symbol(&mut self, realm: Realm) -> Owned<Value> {
        Stub::owned(self.alloc(self.index(realm), Body::Symbol))
    }

    pub(crate) fn is_symbol(&self, v: Value) -> bool {
        // SAFETY: a live cell, read only
        v.is_cell() && matches!(unsafe { Cell::at(v) }.body, Body::Symbol)
    }
}
