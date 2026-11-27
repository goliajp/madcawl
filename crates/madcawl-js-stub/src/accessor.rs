//! Accessor properties the host defines (`Builtins::define_accessor`),
//! deleting properties, and preserving a wrapper script wrote to.

use madcawl_js::{Builtins, Host, Key};

use crate::cell::{Body, Cell, PropKey};
use crate::isolate::{Realm, Stub};
use crate::props::class_as;
use crate::value::{THROWN, UNDEFINED, Value};

/// The getter, setter and whether it is configurable when the own
/// property value `v` is an accessor.
pub(crate) fn accessor(v: Value) -> Option<(Value, Value, bool)> {
    if !v.is_cell() {
        return None;
    }
    // SAFETY: a live cell, read only
    match unsafe { Cell::at(v) }.body {
        Body::Accessor { get, set, configurable } => Some((get, set, configurable)),
        _ => None,
    }
}

impl Stub {
    fn own_accessor(&self, obj: Value, k: PropKey) -> Option<(Value, Value, bool)> {
        // SAFETY: a live cell, read only
        let c = unsafe { Cell::at(obj) };
        c.props.iter().find(|(pk, _)| *pk == k).and_then(|&(_, v)| accessor(v))
    }

    /// A write to own accessor `k` of `obj`: its setter's outcome, or
    /// ignored without one, as outside strict mode; `None` for no such
    /// accessor.
    pub(crate) fn set_accessor<H: Host<Stub>>(&mut self, host: &mut H, obj: Value, k: PropKey, v: Value) -> Option<bool> {
        let (_, set, _) = self.own_accessor(obj, k)?;
        if set == UNDEFINED {
            return Some(true);
        }
        let r = self.call_value(host, set, obj, &[v]);
        let ok = r.get() != THROWN;
        self.release_value(r.into_raw());
        Some(ok)
    }

    pub(crate) fn delete_prop(&mut self, obj: Value, key: Key<'_>) -> bool {
        let obj = self.target(obj);
        if !obj.is_cell() {
            return true;
        }
        let k = self.key(key);
        if self.own_accessor(obj, k).is_some_and(|(_, _, configurable)| !configurable) {
            return true;
        }
        // SAFETY: a live cell
        let c = unsafe { Cell::at(obj) };
        if c.frozen {
            return true;
        }
        if let Some(i) = c.props.iter().position(|(pk, _)| *pk == k) {
            let (_, v) = c.props.remove(i);
            self.release_value(v);
        }
        true
    }

    /// The wrapper `obj` gained state script can observe: the engine
    /// counts the edge from its host object, and tells the host.
    pub(crate) fn preserve<H: Host<Stub>>(&mut self, host: &mut H, obj: Value) {
        if !obj.is_cell() {
            return;
        }
        // SAFETY: a live cell
        let c = unsafe { Cell::at(obj) };
        if let Body::Wrapper { preserved: p @ false, class, obj: node, .. } = &mut c.body {
            *p = true;
            let (class, node) = (*class, *node);
            c.set_count(c.count() + 1);
            (class_as::<H>(&class).host_ops().wrapper_needs_preserve)(host, node);
        }
    }
}

/// The stub has no typed arrays and no error objects of script's; its
/// accessors are own properties of the object, read and written as
/// such.
impl Builtins for Stub {
    fn define_accessor(&mut self, realm: Realm, obj: Value, name: &str, get: Value, set: Value, unforgeable: bool) -> bool {
        let obj = self.target(obj);
        let k = self.key(Key::Str(&madcawl_str::DomString::from_wtf8(name.as_bytes())));
        self.retain_value(get);
        self.retain_value(set);
        let a = self.alloc(self.index(realm), Body::Accessor { get, set, configurable: !unforgeable });
        // SAFETY: a live cell
        let c = unsafe { Cell::at(obj) };
        match c.props.iter().position(|(pk, _)| *pk == k) {
            Some(i) => {
                let old = std::mem::replace(&mut c.props[i].1, a);
                self.release_value(old);
            }
            None => c.props.push((k, a)),
        }
        true
    }
}
