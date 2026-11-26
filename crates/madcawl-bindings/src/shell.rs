//! The C-ABI side of the class tables: each generated `_abi` shell
//! hands its words to one of these, which call the member's Rust glue.
//! They are always inlined into the shell, so that the glue they are
//! passed is a direct call there, not one through a pointer.

use madcawl_js::{Ctx, Engine, Owned};

use crate::convert::arg;
use crate::host::DomHost;

type Value<E> = <E as Engine>::Value;

pub(crate) type Op<E> = fn(&mut E, &mut DomHost<E>, Value<E>, &[Value<E>]) -> Owned<Value<E>>;
pub(crate) type Get<E> = fn(&mut E, &mut DomHost<E>, Value<E>) -> Owned<Value<E>>;
pub(crate) type Set<E> = fn(&mut E, &mut DomHost<E>, Value<E>, Value<E>) -> Owned<Value<E>>;

/// # Safety
/// As for a `HostFn`: `ctx` was made for `E` and `DomHost<E>`, and
/// `argc` words are readable at `argv`.
#[inline(always)]
pub(crate) unsafe fn op<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize, f: Op<E>) -> u64 {
    // SAFETY: the caller's
    let ((e, h), args) = unsafe { (Ctx::parts::<E, DomHost<E>>(ctx), E::values(argv, argc)) };
    E::to_bits(f(e, h, E::from_bits(this), args).into_raw())
}

/// # Safety
/// As for [`op`].
#[inline(always)]
pub(crate) unsafe fn get<E: Engine>(ctx: *mut Ctx, this: u64, f: Get<E>) -> u64 {
    // SAFETY: the caller's
    let (e, h) = unsafe { Ctx::parts::<E, DomHost<E>>(ctx) };
    E::to_bits(f(e, h, E::from_bits(this)).into_raw())
}

/// A setter called without its value gets undefined.
///
/// # Safety
/// As for [`op`].
#[inline(always)]
pub(crate) unsafe fn set<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize, f: Set<E>) -> u64 {
    // SAFETY: the caller's
    let ((e, h), args) = unsafe { (Ctx::parts::<E, DomHost<E>>(ctx), E::values(argv, argc)) };
    E::to_bits(f(e, h, E::from_bits(this), arg::<E>(args, 0)).into_raw())
}

pub(crate) type Index<E> = fn(&mut E, &mut DomHost<E>, Value<E>, u32) -> Owned<Value<E>>;
pub(crate) type Count<E> = fn(&mut E, &mut DomHost<E>, Value<E>) -> u32;

/// # Safety
/// As for [`op`], with no arguments.
#[inline(always)]
pub(crate) unsafe fn index<E: Engine>(ctx: *mut Ctx, this: u64, index: u32, f: Index<E>) -> u64 {
    // SAFETY: the caller's
    let (e, h) = unsafe { Ctx::parts::<E, DomHost<E>>(ctx) };
    E::to_bits(f(e, h, E::from_bits(this), index).into_raw())
}

/// # Safety
/// As for [`index`].
#[inline(always)]
pub(crate) unsafe fn count<E: Engine>(ctx: *mut Ctx, this: u64, f: Count<E>) -> u32 {
    // SAFETY: the caller's
    let (e, h) = unsafe { Ctx::parts::<E, DomHost<E>>(ctx) };
    f(e, h, E::from_bits(this))
}
