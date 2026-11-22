//! Properties, calls, realms and errors.
//!
//! The stub has no prototype objects: a wrapper's class members are
//! found in its static class table, after its own properties.

use std::any::TypeId;

use madcawl_js::{ClassDef, Engine, ErrorKind, Getter, Host, HostFn, Key, Owned, Setter};
use madcawl_str::DomString;

use crate::cell::{Body, Call, Cell, ErasedClass, PropKey};
use crate::isolate::{Realm, RealmData, Stub, name_bytes};
use crate::value::{THROWN, UNDEFINED, Value};

/// A test's script function: `fn(engine, host, captures, this, args)`.
pub type ClosureFn<H> = fn(&mut Stub, &mut H, &[Value], Value, &[Value]) -> Owned<Value>;

enum Member<H: 'static> {
    Get(Getter<Stub, H>),
    Set(Option<Setter<Stub, H>>),
    Method(usize, u16),
    Constant(f64),
}

/// The class table of a wrapper cell, as host type `H`.
pub(crate) fn class_as<H: 'static>(c: &ErasedClass) -> &'static ClassDef<Stub, H> {
    assert_eq!(c.host, TypeId::of::<H>(), "one host type per isolate");
    // SAFETY: erased from this type, checked above; class tables are
    // static
    unsafe { &*(c.def as *const ClassDef<Stub, H>) }
}

/// The class member named `name`, for a read or for a write.
fn member<H: 'static>(
    def: &'static ClassDef<Stub, H>,
    name: &[u8],
    write: bool,
) -> Option<Member<H>> {
    for c in def.chain() {
        if let Some(a) = c.attributes.iter().find(|a| a.name.as_bytes() == name) {
            return Some(if write { Member::Set(a.set) } else { Member::Get(a.get) });
        }
        if let Some(i) = c.operations.iter().position(|o| o.name.as_bytes() == name) {
            return (!write).then_some(Member::Method(std::ptr::from_ref(c) as usize, i as u16));
        }
        if let Some(k) = c.constants.iter().find(|k| k.name.as_bytes() == name) {
            return (!write).then_some(Member::Constant(k.value));
        }
    }
    None
}

impl Stub {
    pub(crate) fn get_prop<H: Host<Stub>>(&mut self, host: &mut H, obj: Value, key: Key<'_>) -> Owned<Value> {
        if !obj.is_cell() {
            if obj == UNDEFINED || obj == crate::value::NULL {
                return self.throw_error(ErrorKind::Type, "cannot read a property of undefined or null");
            }
            return Stub::owned(UNDEFINED);
        }
        let k = self.key(key);
        // SAFETY: a live cell, borrowed for this read only
        let c = unsafe { Cell::at(obj) };
        if let Some(&(_, v)) = c.props.iter().find(|(pk, _)| *pk == k) {
            self.retain_value(v);
            return Stub::owned(v);
        }
        if let (Body::Function { name, length, .. }, PropKey::Name(a)) = (&c.body, k) {
            let (name, length) = (*name, *length);
            return match name_bytes(a).as_slice() {
                b"name" => self.string(DomString::from_wtf8(name.as_bytes())),
                b"length" => Stub::owned(Value::number(length as f64)),
                _ => Stub::owned(UNDEFINED),
            };
        }
        let (Body::Wrapper { class, .. }, realm) = (&c.body, c.realm) else {
            return Stub::owned(UNDEFINED);
        };
        let def = class_as::<H>(class);
        let name = match k {
            PropKey::Name(a) => name_bytes(a),
            PropKey::Index(i) => {
                if let Some(f) = def.chain().find_map(|c| c.interceptors.and_then(|x| x.index)) {
                    return f(self, host, obj, i).unwrap_or(Stub::owned(UNDEFINED));
                }
                return Stub::owned(UNDEFINED);
            }
        };
        match member(def, &name, false) {
            Some(Member::Get(get)) => get(self, host, obj),
            Some(Member::Method(table, i)) => self.method::<H>(realm, table, i),
            Some(Member::Constant(n)) => Stub::owned(Value::number(n)),
            _ => Stub::owned(UNDEFINED),
        }
    }

    pub(crate) fn set_prop<H: Host<Stub>>(&mut self, host: &mut H, obj: Value, key: Key<'_>, v: Value) -> bool {
        if !obj.is_cell() {
            if obj == UNDEFINED || obj == crate::value::NULL {
                let t = self.throw_error(ErrorKind::Type, "cannot set a property of undefined or null");
                self.release_value(t.into_raw());
                return false;
            }
            return true;
        }
        let k = self.key(key);
        // SAFETY: a live cell
        let c = unsafe { Cell::at(obj) };
        if let (Body::Wrapper { class, .. }, PropKey::Name(a)) = (&c.body, k) {
            match member(class_as::<H>(class), &name_bytes(a), true) {
                Some(Member::Set(Some(set))) => {
                    let r = set(self, host, obj, v).into_raw();
                    let ok = r != THROWN;
                    self.release_value(r);
                    return ok;
                }
                // a readonly attribute: ignored, as outside strict mode
                Some(Member::Set(None)) => return true,
                _ => {}
            }
        }
        self.retain_value(v);
        // SAFETY: as above; retain_value touched only `v`
        let c = unsafe { Cell::at(obj) };
        let old = match c.props.iter_mut().find(|(pk, _)| *pk == k) {
            Some(slot) => Some(std::mem::replace(&mut slot.1, v)),
            None => {
                c.props.push((k, v));
                None
            }
        };
        if let Body::Wrapper { preserved: p @ false, class, obj: node, .. } = &mut c.body {
            *p = true;
            let (class, node) = (*class, *node);
            c.set_count(c.count() + 1);
            (class_as::<H>(&class).host_ops().wrapper_needs_preserve)(host, node);
        }
        if let Some(old) = old {
            self.release_value(old);
        }
        true
    }

    pub(crate) fn delete_prop(&mut self, obj: Value, key: Key<'_>) -> bool {
        if !obj.is_cell() {
            return true;
        }
        let k = self.key(key);
        // SAFETY: a live cell
        let c = unsafe { Cell::at(obj) };
        if let Some(i) = c.props.iter().position(|(pk, _)| *pk == k) {
            let (_, v) = c.props.remove(i);
            self.release_value(v);
        }
        true
    }

    fn method<H: Host<Stub>>(&mut self, realm: u32, table: usize, i: u16) -> Owned<Value> {
        let f = match self.realm_data(realm).methods.get(&(table, i)) {
            Some(&f) => f,
            None => {
                // SAFETY: the address of a static class table of host
                // type H, taken in `member`
                let def = unsafe { &*(table as *const ClassDef<Stub, H>) };
                let op = &def.operations[i as usize];
                let f = self.function::<H>(realm, op.name, op.length, op.call).into_raw();
                self.realm_data(realm).methods.insert((table, i), f);
                f
            }
        };
        self.retain_value(f);
        Stub::owned(f)
    }

    pub(crate) fn function<H: Host<Stub>>(&mut self, realm: u32, name: &'static str, length: u8, f: HostFn<Stub, H>) -> Owned<Value> {
        let call = Call::Host { f: f as *const (), host: TypeId::of::<H>() };
        Stub::owned(self.alloc(realm, Body::Function { name, length, call }))
    }

    /// A script function made of a Rust body and the values it closes
    /// over: what a test uses where a page would have a closure.
    pub fn new_closure<H: Host<Stub>>(&mut self, realm: Realm, body: ClosureFn<H>, captures: Vec<Owned<Value>>) -> Owned<Value> {
        let call = Call::Closure {
            body: body as *const (),
            host: TypeId::of::<H>(),
            captures: captures.into_iter().map(Owned::into_raw).collect(),
        };
        Stub::owned(self.alloc(realm.0, Body::Function { name: "", length: 0, call }))
    }

    pub(crate) fn call_value<H: Host<Stub>>(&mut self, host: &mut H, f: Value, this: Value, args: &[Value]) -> Owned<Value> {
        if !self.is_callable(f) {
            return self.throw_error(ErrorKind::Type, "not a function");
        }
        // SAFETY: a live cell
        let c = unsafe { Cell::at(f) };
        let saved = std::mem::replace(&mut self.current, c.realm);
        let Body::Function { call, .. } = &c.body else { unreachable!("callable") };
        let r = match call {
            Call::Host { f, host: t } => {
                assert_eq!(*t, TypeId::of::<H>(), "one host type per isolate");
                // SAFETY: erased from this type, checked above
                let f: HostFn<Stub, H> = unsafe { std::mem::transmute::<*const (), HostFn<Stub, H>>(*f) };
                f(self, host, this, args)
            }
            Call::Closure { body, host: t, captures } => {
                assert_eq!(*t, TypeId::of::<H>(), "one host type per isolate");
                // SAFETY: as above
                let body: ClosureFn<H> = unsafe { std::mem::transmute::<*const (), ClosureFn<H>>(*body) };
                let captures = captures.clone();
                body(self, host, &captures, this, args)
            }
        };
        self.current = saved;
        r
    }

    pub(crate) fn error(&mut self, kind: ErrorKind, message: &str) -> Owned<Value> {
        let name = match kind {
            ErrorKind::Type => "TypeError",
            ErrorKind::Range => "RangeError",
            ErrorKind::Syntax => "SyntaxError",
            ErrorKind::Reference => "ReferenceError",
            ErrorKind::Dom(name) => name,
        };
        let e = self.alloc(self.current, Body::Object);
        for (k, s) in [(b"name".as_slice(), name), (b"message", message)] {
            let v = self.string(DomString::from_wtf8(s.as_bytes())).into_raw();
            let key = crate::cell::PropKey::Name(self.names.hold(k));
            // SAFETY: the cell just made
            unsafe { Cell::at(e) }.props.push((key, v));
        }
        Stub::owned(e)
    }

    pub(crate) fn make_realm(&mut self) -> Realm {
        let r = self.realms.len() as u32;
        let global = self.alloc(r, Body::Object);
        self.realms.push(RealmData { global, detached: false, methods: Default::default() });
        Realm(r)
    }
}

/// Release what a realm holds itself.
pub(crate) fn detach(e: &mut Stub, realm: Realm) {
    let d = e.realm_data(realm.0);
    if d.detached {
        return;
    }
    d.detached = true;
    let mut held: Vec<Value> = d.methods.drain().map(|(_, f)| f).collect();
    held.push(d.global);
    for v in held {
        e.release_value(v);
    }
}
