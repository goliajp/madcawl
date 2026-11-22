//! Host classes: the static tables made from Web IDL.
//!
//! The members' functions are C-ABI ([`HostFn`]), whatever the engine
//! is written in: the bindings generate a thin shell per member that
//! takes the words and calls the Rust glue.

use std::ffi::c_void;
use std::marker::PhantomData;

use crate::engine::Engine;
use crate::value::HostRef;

/// A class id. Ids are given in preorder over the inheritance tree, so
/// the classes derived from a class (and the class itself) are the ids
/// from its own to its `last`: a brand check is one range compare.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClassId(pub u16);

/// What a host function gets besides `this` and its arguments: the
/// isolate and the host it was called with, as the engine passed them
/// down. Made by the engine for one call, valid for that call.
#[repr(C)]
pub struct Ctx {
    pub engine: *mut c_void,
    pub host: *mut c_void,
}

impl Ctx {
    pub fn new<E: Engine, H: 'static>(e: &mut E, h: &mut H) -> Ctx {
        Ctx {
            engine: std::ptr::from_mut(e).cast(),
            host: std::ptr::from_mut(h).cast(),
        }
    }

    /// The isolate and the host.
    ///
    /// # Safety
    /// `ctx` was made by [`Ctx::new`] from an `&mut E` and an `&mut H`
    /// for the call in progress, and the caller does not use those two
    /// while the result is alive.
    pub unsafe fn parts<'a, E, H>(ctx: *mut Ctx) -> (&'a mut E, &'a mut H) {
        // SAFETY: the caller's
        unsafe { (&mut *(*ctx).engine.cast::<E>(), &mut *(*ctx).host.cast::<H>()) }
    }
}

/// A host function, a getter or a setter, as the class tables hold
/// them: the C calling convention, so that compiled code can call the
/// pointer directly.
///
/// `this` and the `argc` words at `argv` are values as
/// [`Engine::to_bits`] gives them, borrowed for the call; a getter gets
/// no arguments and a setter one. The result carries a count, or is
/// the bits of [`Engine::thrown`] with the exception registered in the
/// engine.
///
/// # Safety
/// Called with a `ctx` made from the engine and host types it was
/// bound to ([`Bound`]), and `argc` readable words at `argv`.
pub type HostFn =
    unsafe extern "C" fn(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64;

/// A getter: a [`HostFn`] called with no arguments.
pub type Getter = HostFn;

/// A setter: a [`HostFn`] called with the value as its one argument;
/// its result is undefined or thrown.
pub type Setter = HostFn;

/// `(ctx, this, index) -> value`, under the rules of [`HostFn`], or the
/// bits of [`Engine::absent`] for no such index.
pub type IndexGetter = unsafe extern "C" fn(ctx: *mut Ctx, this: u64, index: u32) -> u64;

/// `(ctx, this) -> how many indices`.
pub type Length = unsafe extern "C" fn(ctx: *mut Ctx, this: u64) -> u32;

/// A C-ABI function bound to engine type `E` and host type `H`: what a
/// class table of `ClassDef<E, H>` and [`Engine::new_function`] take.
/// The function reads its [`Ctx`] as those two types, which nothing in
/// its pointer type says; the binding is where that is asserted, once,
/// so that a table or a function of one host type cannot be handed to
/// another.
///
/// ```
/// use madcawl_js::{Bound, Ctx, HostFn, Operation};
/// struct Engine;
/// struct HostX;
/// unsafe extern "C" fn f(_: *mut Ctx, _: u64, _: *const u64, _: usize) -> u64 {
///     0
/// }
/// // SAFETY: f reads nothing from its context
/// let b: Bound<HostFn, Engine, HostX> = unsafe { Bound::host_fn(f) };
/// let _op: Operation<Engine, HostX> = Operation { name: "f", length: 0, call: b };
/// ```
///
/// A function bound to one host type does not fit a table of another:
///
/// ```compile_fail
/// use madcawl_js::{Bound, Ctx, HostFn, Operation};
/// struct Engine;
/// struct HostX;
/// struct HostY;
/// unsafe extern "C" fn f(_: *mut Ctx, _: u64, _: *const u64, _: usize) -> u64 {
///     0
/// }
/// let b: Bound<HostFn, Engine, HostX> = unsafe { Bound::host_fn(f) };
/// let _op: Operation<Engine, HostY> = Operation { name: "f", length: 0, call: b };
/// ```
///
/// and binding is unsafe:
///
/// ```compile_fail
/// use madcawl_js::{Bound, Ctx, HostFn};
/// struct Engine;
/// struct HostX;
/// unsafe extern "C" fn f(_: *mut Ctx, _: u64, _: *const u64, _: usize) -> u64 {
///     0
/// }
/// let _b: Bound<HostFn, Engine, HostX> = Bound::host_fn(f);
/// ```
pub struct Bound<F, E, H> {
    f: F,
    of: PhantomData<fn(&mut E, &mut H)>,
}

impl<F: Copy, E, H> Bound<F, E, H> {
    pub fn get(&self) -> F {
        self.f
    }
}

impl<F: Copy, E, H> Clone for Bound<F, E, H> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<F: Copy, E, H> Copy for Bound<F, E, H> {}

impl<E, H> Bound<HostFn, E, H> {
    /// # Safety
    /// `f` reads `ctx` as an `E` and an `H` ([`Ctx::parts::<E, H>`]).
    pub const unsafe fn host_fn(f: HostFn) -> Self {
        Bound { f, of: PhantomData }
    }
}

impl<E, H> Bound<IndexGetter, E, H> {
    /// # Safety
    /// As for [`Bound::host_fn`].
    pub const unsafe fn index(f: IndexGetter) -> Self {
        Bound { f, of: PhantomData }
    }
}

impl<E, H> Bound<Length, E, H> {
    /// # Safety
    /// As for [`Bound::host_fn`].
    pub const unsafe fn length(f: Length) -> Self {
        Bound { f, of: PhantomData }
    }
}

pub struct Attribute<E: 'static, H: 'static> {
    pub name: &'static str,
    pub get: Bound<Getter, E, H>,
    /// `None` for a readonly attribute.
    pub set: Option<Bound<Setter, E, H>>,
}

pub struct Operation<E: 'static, H: 'static> {
    pub name: &'static str,
    /// The function's `length`: its required arguments.
    pub length: u8,
    pub call: Bound<HostFn, E, H>,
}

pub struct Constant {
    pub name: &'static str,
    pub value: f64,
}

/// Indexed and named property interception (collections, `window`'s
/// named access). When a getter has no such property it returns the
/// bits of [`Engine::absent`], and lookup continues on the class's
/// static members.
pub struct Interceptors<E: 'static, H: 'static> {
    pub index: Option<Bound<IndexGetter, E, H>>,
    pub length: Option<Bound<Length, E, H>>,
}

/// What the collector and the wrapper protocol ask of a host object.
///
/// `traverse` reports every counted edge out of `obj` exactly once:
/// one reported too few leaks a cycle, one too many frees a live
/// object. It runs on the isolate's thread inside a collection and must
/// not run script, allocate engine objects, make wrappers, change a
/// count or change an edge. `unlink` gives up the edges of an object
/// found to be garbage; the object stays usable until the batch ends,
/// since others in its cycle may still read it. `can_skip` says the
/// object is certainly alive (a node in an active document), so the
/// collector does not enter it.
pub struct HostOps<E: Engine, H: 'static> {
    pub traverse: fn(&H, HostRef, &mut <E as Engine>::Tracer),
    pub unlink: fn(&mut H, HostRef, &mut <E as Engine>::Unlinker),
    pub can_skip: fn(&H, HostRef) -> bool,
    /// The wrapper of `obj` gained state script could observe (an own
    /// property, use as a weak key or target, a changed prototype, a
    /// frozen or sealed object). The engine has added one count to the
    /// wrapper for the edge from `obj`; the host marks the edge as
    /// strong and reports it from then on.
    pub wrapper_needs_preserve: fn(&mut H, HostRef),
    /// The wrapper of `obj` is gone: the host clears its wrapper word and
    /// gives up the count the wrapper held on `obj`.
    pub wrapper_died: fn(&mut H, HostRef),
}

pub struct ClassDef<E: Engine, H: 'static> {
    pub name: &'static str,
    pub id: ClassId,
    /// The highest id among the classes derived from this one.
    pub last: ClassId,
    pub parent: Option<&'static ClassDef<E, H>>,
    pub attributes: &'static [Attribute<E, H>],
    pub operations: &'static [Operation<E, H>],
    pub constants: &'static [Constant],
    pub interceptors: Option<&'static Interceptors<E, H>>,
    /// The host object callbacks; a class without them uses its
    /// parent's.
    pub ops: Option<&'static HostOps<E, H>>,
}

impl<E: Engine, H: 'static> ClassDef<E, H> {
    /// Whether an object of class `id` is an instance of this class.
    pub fn contains(&self, id: ClassId) -> bool {
        self.id <= id && id <= self.last
    }

    pub fn host_ops(&self) -> &'static HostOps<E, H> {
        let mut c = self;
        loop {
            if let Some(ops) = c.ops {
                return ops;
            }
            c = c.parent.expect("a host class with host object callbacks");
        }
    }

    /// This class and its ancestors, nearest first.
    pub fn chain(&'static self) -> impl Iterator<Item = &'static ClassDef<E, H>> {
        std::iter::successors(Some(self), |c| c.parent)
    }
}
