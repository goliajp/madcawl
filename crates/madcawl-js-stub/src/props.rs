//! Properties, calls, realms and errors.
//!
//! A name is looked up along the prototype chain (`proto.rs`): own
//! properties, then the members each host class on the chain declares.

use std::any::TypeId;

use madcawl_js::{Bound, ClassDef, Ctx, Engine, ErrorKind, Getter, Host, HostFn, Key, Owned, Setter};
use madcawl_str::DomString;

use crate::accessor::accessor;
use crate::cell::{Body, Call, Cell, ErasedClass, PropKey};
use crate::isolate::{Realm, RealmData, Stub, name_bytes};
use crate::proto::Link;
use crate::value::{THROWN, UNDEFINED, Value};

/// A test's script function: `fn(engine, host, captures, this, args)`.
pub type ClosureFn<H> = fn(&mut Stub, &mut H, &[Value], Value, &[Value]) -> Owned<Value>;

enum Member {
    Get(Getter),
    Set(Option<Setter>),
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

/// The member named `name` that class `c` itself declares, for a read
/// or for a write.
fn member<H: 'static>(c: &'static ClassDef<Stub, H>, name: &[u8], write: bool) -> Option<Member> {
    if let Some(a) = c.attributes.iter().find(|a| a.name.as_bytes() == name) {
        return Some(if write { Member::Set(a.set.map(|f| f.get())) } else { Member::Get(a.get.get()) });
    }
    if let Some(i) = c.operations.iter().position(|o| o.name.as_bytes() == name) {
        return (!write).then_some(Member::Method(std::ptr::from_ref(c) as usize, i as u16));
    }
    if let Some(k) = c.constants.iter().find(|k| k.name.as_bytes() == name) {
        return (!write).then_some(Member::Constant(k.value));
    }
    None
}

impl Stub {
    pub(crate) fn get_prop<H: Host<Stub>>(&mut self, host: &mut H, obj: Value, key: Key<'_>) -> Owned<Value> {
        let obj = self.target(obj);
        if !obj.is_cell() {
            if obj == UNDEFINED || obj == crate::value::NULL {
                return self.throw_error(ErrorKind::Type, "cannot read a property of undefined or null");
            }
            return Stub::owned(UNDEFINED);
        }
        // SAFETY: a live cell, read only
        self.own(unsafe { Cell::at(obj) });
        let k = self.key(key);
        if let PropKey::Index(i) = k {
            return self.get_index(host, obj, k, i);
        }
        let PropKey::Name(a) = k else { unreachable!() };
        let name = name_bytes(a);
        for link in self.chain::<H>(obj) {
            match link {
                Link::Own(o) => {
                    // SAFETY: a live cell on the chain, read only
                    let c = unsafe { Cell::at(o) };
                    if let Some(&(_, v)) = c.props.iter().find(|(pk, _)| *pk == k) {
                        if let Some((get, _, _)) = accessor(v) {
                            return self.call_value(host, get, obj, &[]);
                        }
                        self.retain_value(v);
                        return Stub::owned(v);
                    }
                    if let Some(v) = self.virtual_prop::<H>(o, &name) {
                        return v;
                    }
                }
                Link::Class(def, realm) => match member(def, &name, false) {
                    Some(Member::Get(get)) => return Stub::owned(self.invoke_in(host, realm, get, obj, &[])),
                    Some(Member::Method(table, i)) => return self.method::<H>(realm, table, i),
                    Some(Member::Constant(n)) => return Stub::owned(Value::number(n)),
                    _ => {}
                },
            }
        }
        Stub::owned(UNDEFINED)
    }

    /// An index: an own property, then the receiver's index getter.
    fn get_index<H: Host<Stub>>(&mut self, host: &mut H, obj: Value, k: PropKey, i: u32) -> Owned<Value> {
        // SAFETY: a live cell, read only
        let c = unsafe { Cell::at(obj) };
        if let Some(&(_, v)) = c.props.iter().find(|(pk, _)| *pk == k) {
            self.retain_value(v);
            return Stub::owned(v);
        }
        let Body::Wrapper { class, .. } = &c.body else { return Stub::owned(UNDEFINED) };
        let def = class_as::<H>(class);
        let Some(f) = def.chain().find_map(|c| c.interceptors.and_then(|x| x.index)).map(|f| f.get()) else {
            return Stub::owned(UNDEFINED);
        };
        let r = self.host_call(host, 0, |ctx| {
            // SAFETY: a table entry of host type H, called with a context
            // for H
            unsafe { f(ctx, obj.0, i) }
        });
        Stub::owned(if r == crate::value::ABSENT { UNDEFINED } else { r })
    }

    pub(crate) fn set_prop<H: Host<Stub>>(&mut self, host: &mut H, obj: Value, key: Key<'_>, v: Value) -> bool {
        let obj = self.target(obj);
        if !obj.is_cell() {
            if obj == UNDEFINED || obj == crate::value::NULL {
                let t = self.throw_error(ErrorKind::Type, "cannot set a property of undefined or null");
                self.release_value(t.into_raw());
                return false;
            }
            return true;
        }
        // SAFETY: a live cell, read only
        self.own(unsafe { Cell::at(obj) });
        let k = self.key(key);
        if let Some(r) = self.set_accessor(host, obj, k, v) {
            return r;
        }
        if let PropKey::Name(a) = k {
            let name = name_bytes(a);
            let setter = self.chain::<H>(obj).into_iter().find_map(|l| match l {
                Link::Class(def, realm) => member(def, &name, true).map(|m| (m, realm)),
                Link::Own(_) => None,
            });
            match setter {
                Some((Member::Set(Some(set)), realm)) => {
                    let r = self.invoke_in(host, realm, set, obj, &[v]);
                    let ok = r != THROWN;
                    self.release_value(r);
                    return ok;
                }
                // a readonly attribute: ignored, as outside strict mode
                Some((Member::Set(None), _)) => return true,
                _ => {}
            }
        }
        // SAFETY: a live cell
        let c = unsafe { Cell::at(obj) };
        let slot = c.props.iter().position(|(pk, _)| *pk == k);
        // a frozen object takes no write, a non-extensible one no new
        // property: ignored, as outside strict mode
        if c.frozen || (slot.is_none() && !c.extensible) {
            return true;
        }
        self.retain_value(v);
        // SAFETY: as above; retain_value touched only `v`
        let c = unsafe { Cell::at(obj) };
        let old = match slot {
            Some(i) => Some(std::mem::replace(&mut c.props[i].1, v)),
            None => {
                c.props.push((k, v));
                None
            }
        };
        self.preserve(host, obj);
        if let Some(old) = old {
            self.release_value(old);
        }
        true
    }

    fn method<H: Host<Stub>>(&mut self, realm: u32, table: usize, i: u16) -> Owned<Value> {
        // SAFETY: the address of a static class table of host type H,
        // taken in `member`
        let def = unsafe { &*(table as *const ClassDef<Stub, H>) };
        let f = match self.realm_data(realm).methods.get(&(def.id.0, i)) {
            Some(&f) => f,
            None => {
                let op = &def.operations[i as usize];
                let f = self.function::<H>(realm, op.name, op.length, op.call, 0).into_raw();
                self.realm_data(realm).methods.insert((def.id.0, i), f);
                f
            }
        };
        self.retain_value(f);
        Stub::owned(f)
    }

    pub(crate) fn function<H: Host<Stub>>(&mut self, realm: u32, name: &'static str, length: u8, f: Bound<HostFn, Stub, H>, data: u64) -> Owned<Value> {
        // the binding to H is checked again at each call, by the type id
        let call = Call::Host { f: f.get(), host: TypeId::of::<H>(), data };
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
        Stub::owned(self.alloc(self.index(realm), Body::Function { name: "", length: 0, call }))
    }

    pub(crate) fn call_value<H: Host<Stub>>(&mut self, host: &mut H, f: Value, this: Value, args: &[Value]) -> Owned<Value> {
        self.deliver(host);
        if !self.is_callable(f) {
            return self.throw_error(ErrorKind::Type, "not a function");
        }
        // SAFETY: a live cell
        let c = unsafe { Cell::at(f) };
        self.own(c);
        let saved = std::mem::replace(&mut self.current, c.realm);
        let Body::Function { call, .. } = &c.body else { unreachable!("callable") };
        let r = match call {
            Call::Host { f, host: t, data } => {
                assert_eq!(*t, TypeId::of::<H>(), "one host type per isolate");
                let (f, data) = (*f, *data);
                Stub::owned(self.invoke(host, f, this, args, data))
            }
            Call::Interface { .. } => self.throw_error(ErrorKind::Type, "illegal constructor"),
            Call::Closure { body, host: t, captures } => {
                assert_eq!(*t, TypeId::of::<H>(), "one host type per isolate");
                // SAFETY: erased from this type, checked above
                let body: ClosureFn<H> = unsafe { std::mem::transmute::<*const (), ClosureFn<H>>(*body) };
                let captures = captures.clone();
                if self.enter() {
                    let r = body(self, host, &captures, this, args);
                    Stub::owned(self.leave(r.into_raw()))
                } else {
                    Stub::thrown()
                }
            }
        };
        self.current = saved;
        r
    }

    /// Call a host function from a class table or a function cell.
    pub(crate) fn invoke<H: Host<Stub>>(&mut self, host: &mut H, f: HostFn, this: Value, args: &[Value], data: u64) -> Value {
        self.host_call(host, data, |ctx| {
            // SAFETY: a host function of host type H, called with a context
            // for H; `Value` is a transparent word
            unsafe { f(ctx, this.0, args.as_ptr().cast(), args.len()) }
        })
    }

    /// Enter host code through `call`, counted in the script depth.
    fn host_call<H: Host<Stub>>(&mut self, host: &mut H, data: u64, call: impl FnOnce(*mut Ctx) -> u64) -> Value {
        if !self.enter() {
            return crate::value::THROWN;
        }
        let mut ctx = Ctx::new(self, host, data);
        let r = Value(call(&mut ctx));
        self.leave(r)
    }

    pub(crate) fn error(&mut self, kind: ErrorKind, message: &str) -> Owned<Value> {
        let name = match kind {
            ErrorKind::Type => "TypeError",
            ErrorKind::Range => "RangeError",
            ErrorKind::Syntax => "SyntaxError",
            ErrorKind::Reference => "ReferenceError",
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
        self.realms.push(RealmData { global, detached: false, methods: Default::default(), protos: Default::default() });
        Realm(r, self.id)
    }
}

/// Release what a realm holds itself.
pub(crate) fn detach(e: &mut Stub, realm: Realm) {
    let d = e.realm_data(e.index(realm));
    if d.detached {
        return;
    }
    d.detached = true;
    let mut held: Vec<Value> = d.methods.drain().map(|(_, f)| f).collect();
    held.extend(d.protos.drain().flat_map(|(_, (i, p))| [i, p]));
    held.push(d.global);
    for v in held {
        e.release_value(v);
    }
}
