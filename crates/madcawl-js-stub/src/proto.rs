//! Prototype chains: each host class has an interface object and a
//! prototype object per realm, made when first asked for; a wrapper
//! inherits from its class's. Plain objects and functions have no
//! prototype (there is no `Object.prototype` here), unless script set
//! one.
//!
//! A class whose prototype object was never made still contributes its
//! members to the chain, so reading a member makes no object.

use madcawl_js::{ClassDef, Engine, Host, Owned};
use madcawl_str::DomString;

use crate::cell::{Body, Call, Cell, ErasedClass};
use crate::isolate::Stub;
use crate::props::class_as;
use crate::value::{NULL, UNDEFINED, Value};

/// One step of a chain: a cell's own properties, or the members one
/// class declares (on its prototype object of `realm`).
pub(crate) enum Link<H: 'static> {
    Own(Value),
    Class(&'static ClassDef<Stub, H>, u32),
}

enum Next<H: 'static> {
    Cell(Value),
    Proto(Option<&'static ClassDef<Stub, H>>, u32),
}

impl Stub {
    /// The chain from `obj`, a live cell, to the end.
    pub(crate) fn chain<H: 'static>(&self, obj: Value) -> Vec<Link<H>> {
        let mut out = Vec::new();
        let mut next = Next::Cell(obj);
        loop {
            next = match next {
                Next::Cell(o) => {
                    out.push(Link::Own(o));
                    // SAFETY: a live cell on the chain, read only
                    let c = unsafe { Cell::at(o) };
                    let default = match &c.body {
                        Body::Wrapper { class, .. } => Next::Proto(Some(class_as::<H>(class)), c.realm),
                        Body::Prototype { class } => {
                            let def = class_as::<H>(class);
                            out.push(Link::Class(def, c.realm));
                            Next::Proto(def.parent, c.realm)
                        }
                        _ => Next::Proto(None, c.realm),
                    };
                    match c.proto {
                        Some(p) if p.is_cell() => Next::Cell(p),
                        Some(_) => break,
                        None => default,
                    }
                }
                Next::Proto(Some(def), realm) => {
                    match self.realms[realm as usize].protos.get(&def.id.0) {
                        Some(&(_, p)) => Next::Cell(p),
                        None => {
                            out.push(Link::Class(def, realm));
                            Next::Proto(def.parent, realm)
                        }
                    }
                }
                Next::Proto(None, _) => break,
            };
        }
        out
    }

    /// A property a cell has by its kind: a function's `name` and
    /// `length`, an interface object's `prototype`, a prototype
    /// object's `constructor`.
    pub(crate) fn virtual_prop<H: Host<Stub>>(&mut self, o: Value, name: &[u8]) -> Option<Owned<Value>> {
        // SAFETY: a live cell, read only
        let c = unsafe { Cell::at(o) };
        let realm = c.realm;
        match (&c.body, name) {
            (Body::Function { name: n, .. }, b"name") => {
                let n = *n;
                Some(self.string(DomString::from_wtf8(n.as_bytes())))
            }
            (Body::Function { length, .. }, b"length") => Some(Stub::owned(Value::number(*length as f64))),
            (Body::Function { call: Call::Interface { class }, .. }, b"prototype") => {
                let (_, p) = self.class_objects(realm, class_as::<H>(class));
                Some(self.retain(p))
            }
            // static operations and constants are on the interface object
            // too, and an interface object inherits its parent's
            (Body::Function { call: Call::Interface { class }, .. }, _) => {
                let def = class_as::<H>(class);
                if let Some((c, i)) = def.chain().find_map(|c| c.statics.iter().position(|o| o.name.as_bytes() == name).map(|i| (c, i))) {
                    return Some(self.static_op::<H>(realm, c, i));
                }
                let k = def.chain().flat_map(|c| c.constants).find(|k| k.name.as_bytes() == name)?;
                Some(Stub::owned(Value::number(k.value)))
            }
            (Body::Prototype { class }, b"constructor") => {
                let (i, _) = self.class_objects(realm, class_as::<H>(class));
                Some(self.retain(i))
            }
            _ => None,
        }
    }

    /// Static operation `i` of `def` as a function, the same one on
    /// every read; it shares the methods table by an index no operation
    /// or getter has.
    fn static_op<H: Host<Stub>>(&mut self, realm: u32, def: &'static ClassDef<Stub, H>, i: usize) -> Owned<Value> {
        let key = (def.id.0, 0x4000 | i as u16);
        let f = match self.realm_data(realm).methods.get(&key) {
            Some(&f) => f,
            None => {
                let o = &def.statics[i];
                let f = self.function::<H>(realm, o.name, o.length, o.call, 0).into_raw();
                self.realm_data(realm).methods.insert(key, f);
                f
            }
        };
        self.retain(f)
    }

    /// The interface object and prototype object of `class` in `realm`,
    /// borrowed from the realm, which holds them.
    pub(crate) fn class_objects<H: 'static>(&mut self, realm: u32, class: &'static ClassDef<Stub, H>) -> (Value, Value) {
        let key = class.id.0;
        if let Some(&pair) = self.realm_data(realm).protos.get(&key) {
            return pair;
        }
        let erased = ErasedClass::of(class);
        let call = Call::Interface { class: erased };
        let iface = self.alloc(realm, Body::Function { name: class.name, length: 0, call });
        let proto = self.alloc(realm, Body::Prototype { class: erased });
        self.realm_data(realm).protos.insert(key, (iface, proto));
        (iface, proto)
    }

    /// `Object.getPrototypeOf(v)`: null at the end of a chain.
    pub fn prototype_of<H: 'static>(&mut self, v: Value) -> Owned<Value> {
        if !v.is_cell() {
            return Stub::owned(NULL);
        }
        // SAFETY: a live cell, read only
        let c = unsafe { Cell::at(v) };
        let p = match (&c.body, c.proto) {
            (_, Some(p)) => p,
            (Body::Wrapper { class, .. }, None) => {
                let (realm, def) = (c.realm, class_as::<H>(class));
                self.class_objects(realm, def).1
            }
            (Body::Prototype { class }, None) => match class_as::<H>(class).parent {
                Some(def) => {
                    let realm = c.realm;
                    self.class_objects(realm, def).1
                }
                None => NULL,
            },
            _ => NULL,
        };
        self.retain(p)
    }

    /// `Object.setPrototypeOf(obj, proto)`; `proto` is an object or
    /// null. False when `obj` is not extensible or a cycle would form.
    pub fn set_prototype<H: Host<Stub>>(&mut self, host: &mut H, obj: Value, proto: Value) -> bool {
        assert!(obj.is_cell() && (proto.is_cell() || proto == NULL), "an object and an object or null");
        let mut p = proto;
        while p.is_cell() {
            if p == obj {
                return false;
            }
            let next = self.prototype_of::<H>(p);
            p = next.get();
            self.release(next);
        }
        // SAFETY: a live cell
        if !unsafe { Cell::at(obj) }.extensible {
            return false;
        }
        self.retain_value(proto);
        // SAFETY: as above
        let old = unsafe { Cell::at(obj) }.proto.replace(proto);
        self.preserve(host, obj);
        if let Some(old) = old {
            self.release_value(old);
        }
        true
    }

    /// `v instanceof ctor` for an interface object `ctor`.
    pub fn instance_of<H: Host<Stub>>(&mut self, host: &mut H, v: Value, ctor: Value) -> bool {
        let p = self.get_prop(host, ctor, madcawl_js::Key::Str(&DomString::from_wtf8(b"prototype")));
        assert!(p.get().is_cell(), "an interface object");
        let mut o = self.prototype_of::<H>(v);
        let found = loop {
            if o.get() == p.get() {
                break true;
            }
            if !o.get().is_cell() {
                break false;
            }
            let next = self.prototype_of::<H>(o.get());
            self.release(std::mem::replace(&mut o, next));
        };
        self.release(o);
        self.release(p);
        found
    }

    /// `Object.getOwnPropertyDescriptor(proto, name).get` for prototype
    /// object `proto`: the getter of the attribute its class declares,
    /// as a function, the same one on every read; undefined when it has
    /// no such attribute.
    pub fn own_getter<H: Host<Stub>>(&mut self, proto: Value, name: &str) -> Owned<Value> {
        // SAFETY: a live cell, read only
        let c = unsafe { Cell::at(proto) };
        let Body::Prototype { class } = &c.body else {
            return Stub::owned(UNDEFINED);
        };
        let (realm, def) = (c.realm, class_as::<H>(class));
        let Some(i) = def.attributes.iter().position(|a| a.name == name) else {
            return Stub::owned(UNDEFINED);
        };
        // getters share the methods table, by an index no operation has
        let key = (def.id.0, 0x8000 | i as u16);
        let f = match self.realm_data(realm).methods.get(&key) {
            Some(&f) => f,
            None => {
                let a = &def.attributes[i];
                let f = self.function::<H>(realm, a.name, 0, a.get, 0).into_raw();
                self.realm_data(realm).methods.insert(key, f);
                f
            }
        };
        self.retain(f)
    }
}
