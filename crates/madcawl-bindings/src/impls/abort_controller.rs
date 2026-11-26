//! The AbortController interface: a signal, held by a count, whose
//! wrapper is a member of the controller's ([SameObject], `holds.rs`).

use madcawl_js::Engine;

use crate::convert::Thrown;
use crate::event::{Held, Target};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

pub(crate) fn constructor<E: Engine>(_: &mut E, h: &mut DomHost<E>) -> Result<SideRef, Thrown> {
    let s = h.new_signal();
    let held = h.hold_target(Target::Side(s));
    Ok(h.sides.add(SideData::Controller(held)))
}

pub(crate) fn signal<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<SideRef, Thrown> {
    match &h.sides.get(this).data {
        SideData::Controller(Held::Side(s)) => Ok(*s),
        _ => unreachable!("a controller's side object holds its signal"),
    }
}

pub(crate) fn abort<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, reason: E::Value) -> Result<(), Thrown> {
    let s = signal(e, h, this)?;
    h.signal_abort(e, s, (reason != E::undefined()).then_some(reason));
    Ok(())
}
