//! The stub's objects: counted cells with the engine object header at
//! offset 0, as every block the engine and the DOM share has.

use std::any::TypeId;

use madcawl_str::abi;
use madcawl_js::{ClassDef, ClassId, HostFn, HostRef};
use madcawl_str::Atom;

use crate::value::Value;

/// The type tag of every stub cell; strings are `abi::TAG_STR`.
pub(crate) const TAG_CELL: u16 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PropKey {
    Name(Atom),
    Index(u32),
}

pub(crate) type Props = Vec<(PropKey, Value)>;

/// A host class table with its host type erased; the type is checked
/// before the table is used as that type again.
#[derive(Clone, Copy)]
pub(crate) struct ErasedClass {
    pub def: *const (),
    pub host: TypeId,
    pub id: ClassId,
}

impl ErasedClass {
    pub(crate) fn of<H: 'static>(def: &'static ClassDef<crate::Stub, H>) -> ErasedClass {
        ErasedClass { def: std::ptr::from_ref(def).cast(), host: TypeId::of::<H>(), id: def.id }
    }
}

pub(crate) enum Call {
    Host { f: HostFn, host: TypeId, data: u64 },
    /// An interface object: calling it throws.
    Interface { class: ErasedClass },
    /// A test's stand-in for a script function: a Rust body and the
    /// values it closes over, which the cell holds.
    Closure {
        body: *const (),
        host: TypeId,
        captures: Vec<Value>,
    },
}

pub(crate) enum Body {
    Object,
    /// A string that is a view into flat string `s`, which it holds.
    StrView {
        s: Value,
        start: u32,
        len: u32,
    },
    /// A WindowProxy, holding the global it forwards to.
    Proxy {
        global: Value,
        realm: u32,
    },
    /// The prototype object of a host class in its realm.
    Prototype { class: ErasedClass },
    /// A script `WeakRef`: its target is in weak handle slot `slot`.
    WeakRef { slot: u32 },
    /// A script `WeakMap`: each key in a weak handle slot, each value
    /// held. An entry whose key died is dropped at the next use.
    WeakMap { entries: Vec<(u32, Value)> },
    Symbol,
    Function {
        name: &'static str,
        length: u8,
        call: Call,
    },
    Wrapper {
        class: ErasedClass,
        obj: HostRef,
        /// The engine counted the edge from the host object.
        preserved: bool,
        /// The count reached 0 and the host has not been told: the
        /// cell stays, still holding its host object, until it is
        /// (`drain_host`). Its memory is not reused before that, so the
        /// host may still read the wrapper word.
        dead: bool,
        /// The collector found the cell to be garbage; it is being
        /// unlinked, and `wrapper_upgrade` no longer gives it out.
        condemned: bool,
        /// `wrapper_upgrade` refused it and the host let go of it: the
        /// wrapper word no longer names it, its edge to the host object
        /// is no longer counted, and it gets no `wrapper_died`.
        orphan: bool,
    },
}

#[repr(C)]
pub(crate) struct Cell {
    pub header: u64,
    pub realm: u32,
    /// The isolate that made it: a value never crosses to another.
    pub isolate: u32,
    pub props: Props,
    /// A prototype script set, held (null included); `None` for the
    /// one the cell's kind gives it.
    pub proto: Option<Value>,
    pub extensible: bool,
    pub frozen: bool,
    pub body: Body,
}

impl Cell {
    pub(crate) fn alloc(isolate: u32, realm: u32, body: Body) -> Value {
        let p = Box::into_raw(Box::new(Cell {
            header: abi::header(TAG_CELL, 0),
            realm,
            isolate,
            props: Vec::new(),
            proto: None,
            extensible: true,
            frozen: false,
            body,
        }));
        Value(p as u64)
    }

    /// # Safety
    /// `v` is a live cell, and no other reference to it is used while
    /// the result is.
    #[allow(clippy::mut_from_ref)]
    pub(crate) unsafe fn at<'a>(v: Value) -> &'a mut Cell {
        // SAFETY: the caller's
        unsafe { &mut *(v.0 as *mut Cell) }
    }

    /// # Safety
    /// `v` is a live cell no one refers to any more.
    pub(crate) unsafe fn free(v: Value) {
        // SAFETY: made by alloc
        drop(unsafe { Box::from_raw(v.0 as *mut Cell) });
    }

    pub(crate) fn count(&self) -> u32 {
        abi::refcount(self.header)
    }

    pub(crate) fn set_count(&mut self, n: u32) {
        self.header = abi::with_refcount(self.header, n);
    }

    /// The values this cell holds a count on.
    pub(crate) fn held(&self, out: &mut Vec<Value>) {
        out.extend(self.props.iter().map(|&(_, v)| v));
        out.extend(self.proto);
        match &self.body {
            Body::WeakMap { entries } => out.extend(entries.iter().map(|&(_, v)| v)),
            Body::Function {
                call: Call::Closure { captures, .. },
                ..
            } => out.extend_from_slice(captures),
            Body::StrView { s: v, .. } | Body::Proxy { global: v, .. } => out.push(*v),
            _ => {}
        }
    }

    /// Give up every value this cell holds, into `out`.
    pub(crate) fn take_held(&mut self, out: &mut Vec<Value>) {
        out.extend(self.props.drain(..).map(|(_, v)| v));
        out.extend(self.proto.take());
        match &mut self.body {
            Body::WeakMap { entries } => out.extend(entries.iter_mut().map(|(_, v)| std::mem::replace(v, crate::value::UNDEFINED))),
            Body::Function {
                call: Call::Closure { captures, .. },
                ..
            } => out.append(captures),
            Body::StrView { s: v, .. } | Body::Proxy { global: v, .. } => {
                out.push(std::mem::replace(v, crate::value::UNDEFINED));
            }
            _ => {}
        }
    }
}
