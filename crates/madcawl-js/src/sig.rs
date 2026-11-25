//! What a class member's Web IDL declaration says about it, for an
//! engine that calls members on a fast path. The generator fills it in
//! from the IDL; an engine that ignores it is still correct, since the
//! member's function checks and converts everything itself.

/// A Web IDL type, as coarsely as compiled code can use it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdlType {
    Undefined,
    Boolean,
    /// Any numeric type.
    Number,
    String,
    NullableString,
    /// An interface type: a wrapper of that interface.
    Object,
    NullableObject,
    /// A callback function or interface, or null.
    Callback,
    Any,
}

/// Effect bits of a member. A bit that is clear is a promise the
/// member keeps whenever its receiver passed the brand check; a bit
/// that is set promises nothing. RUNS_SCRIPT voids every other
/// promise: the script it runs (an argument's toString) may change the
/// DOM or throw whatever the member's own bits say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effects(pub u8);

impl Effects {
    pub const NONE: Effects = Effects(0);
    /// Converting an argument may run script (ToString of an object),
    /// or the member may run script itself (custom element reactions).
    pub const RUNS_SCRIPT: Effects = Effects(1);
    /// It may throw: a missing argument, an argument of the wrong type,
    /// a `DOMException`.
    pub const THROWS: Effects = Effects(2);
    /// Each call returns a new object (`[NewObject]`).
    pub const NEW_OBJECT: Effects = Effects(4);
    /// It may change the DOM: what an earlier read returned may be
    /// stale after it.
    pub const WRITES: Effects = Effects(8);

    pub const fn with(self, other: Effects) -> Effects {
        Effects(self.0 | other.0)
    }

    pub const fn contains(self, other: Effects) -> bool {
        self.0 & other.0 == other.0
    }
}

/// An operation's declaration: its arguments in order, its result and
/// its effects.
#[derive(Clone, Copy, Debug)]
pub struct Signature {
    pub args: &'static [IdlType],
    pub result: IdlType,
    pub effects: Effects,
}

/// An attribute's declaration: its type, and the effects of its getter
/// and of its setter (none for a readonly attribute).
#[derive(Clone, Copy, Debug)]
pub struct AttributeSig {
    pub ty: IdlType,
    pub get: Effects,
    pub set: Effects,
}
