//! The stub's objects: counted cells with the engine object header at
//! offset 0, as every block the engine and the DOM share has.

use std::any::TypeId;

use madcawl_abi as abi;
use madcawl_js::{ClassId, HostRef};
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

pub(crate) enum Call {
    Host { f: *const (), host: TypeId },
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
        /// (`drain_host`) or until it is read again from the wrapper
        /// word (`retain`).
        dead: bool,
        /// A notice for this cell is queued.
        noticed: bool,
    },
}

#[repr(C)]
pub(crate) struct Cell {
    pub header: u64,
    pub realm: u32,
    pub props: Props,
    pub body: Body,
}

impl Cell {
    pub(crate) fn alloc(realm: u32, body: Body) -> Value {
        let p = Box::into_raw(Box::new(Cell {
            header: abi::header(TAG_CELL, 0),
            realm,
            props: Vec::new(),
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
        if let Body::Function {
            call: Call::Closure { captures, .. },
            ..
        } = &self.body
        {
            out.extend_from_slice(captures);
        }
    }

    /// Give up every value this cell holds, into `out`.
    pub(crate) fn take_held(&mut self, out: &mut Vec<Value>) {
        out.extend(self.props.drain(..).map(|(_, v)| v));
        if let Body::Function {
            call: Call::Closure { captures, .. },
            ..
        } = &mut self.body
        {
            out.append(captures);
        }
    }
}
