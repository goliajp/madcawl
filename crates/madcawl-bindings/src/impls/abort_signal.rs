//! The AbortSignal interface's members (`abort.rs` has the signal).

use madcawl_js::Engine;
use madcawl_str::atom;

use crate::convert::{Any, Thrown};
use crate::event::Target;
use crate::host::DomHost;
use crate::side::SideRef;

/// `AbortSignal.abort(reason)`: a signal aborted already.
pub(crate) fn abort<E: Engine>(e: &mut E, h: &mut DomHost<E>, reason: E::Value) -> Result<SideRef, Thrown> {
    let reason = (reason != E::undefined()).then_some(reason);
    Ok(h.aborted_signal(e, reason))
}

pub(crate) fn timeout<E: Engine>(e: &mut E, h: &mut DomHost<E>, ms: u64) -> Result<SideRef, Thrown> {
    let realm = h.caller_realm(e)?;
    let w = h.window_of_realm(realm).expect("a live window's realm");
    Ok(h.timeout_signal(e, ms, w))
}

pub(crate) fn aborted<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<bool, Thrown> {
    Ok(h.signal(this).aborted)
}

pub(crate) fn reason<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
    Ok(Any(h.reason(e, this)))
}

pub(crate) fn throw_if_aborted<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<(), Thrown> {
    if !h.signal(this).aborted {
        return Ok(());
    }
    let reason = h.reason(e, this);
    let _sentinel = e.throw(reason).into_raw();
    Err(Thrown)
}

pub(crate) fn onabort<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
    Ok(Any(h.event_handler(e, Target::Side(this), atom!("abort"))))
}

pub(crate) fn set_onabort<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, value: Option<E::Value>) -> Result<(), Thrown> {
    h.set_event_handler(e, Target::Side(this), atom!("abort"), value);
    Ok(())
}
