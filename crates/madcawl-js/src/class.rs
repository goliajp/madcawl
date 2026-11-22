//! Host classes: the static tables made from Web IDL.

use crate::engine::Engine;
use crate::value::{HostRef, Owned};

/// A class id. Ids are given in preorder over the inheritance tree, so
/// the classes derived from a class (and the class itself) are the ids
/// from its own to its `last`: a brand check is one range compare.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClassId(pub u16);

/// `fn(engine, host, this, args) -> result`: `this` and `args` are
/// borrowed for the call; the result carries a count, or is
/// [`Engine::thrown`] with the exception registered in the engine.
pub type HostFn<E, H> =
    fn(&mut E, &mut H, <E as Engine>::Value, &[<E as Engine>::Value]) -> Owned<<E as Engine>::Value>;

/// `fn(engine, host, this) -> value`, under the same rules.
pub type Getter<E, H> =
    fn(&mut E, &mut H, <E as Engine>::Value) -> Owned<<E as Engine>::Value>;

/// `fn(engine, host, this, value) -> undefined or thrown`.
pub type Setter<E, H> =
    fn(&mut E, &mut H, <E as Engine>::Value, <E as Engine>::Value) -> Owned<<E as Engine>::Value>;

pub struct Attribute<E: Engine, H: 'static> {
    pub name: &'static str,
    pub get: Getter<E, H>,
    /// `None` for a readonly attribute.
    pub set: Option<Setter<E, H>>,
}

pub struct Operation<E: Engine, H: 'static> {
    pub name: &'static str,
    /// The function's `length`: its required arguments.
    pub length: u8,
    pub call: HostFn<E, H>,
}

pub struct Constant {
    pub name: &'static str,
    pub value: f64,
}

/// Indexed and named property interception (collections, `window`'s
/// named access). A getter returns `None` when it has no such property,
/// and lookup continues on the class's static members.
pub struct Interceptors<E: Engine, H: 'static> {
    pub index: Option<IndexGetter<E, H>>,
    pub length: Option<Length<E, H>>,
}

/// `fn(engine, host, this, index) -> value`, `None` for no such index.
pub type IndexGetter<E, H> =
    fn(&mut E, &mut H, <E as Engine>::Value, u32) -> Option<Owned<<E as Engine>::Value>>;

/// `fn(engine, host, this) -> how many indices`.
pub type Length<E, H> = fn(&mut E, &mut H, <E as Engine>::Value) -> u32;

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
