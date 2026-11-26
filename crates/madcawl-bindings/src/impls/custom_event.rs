//! The CustomEvent interface: an event with a `detail` script value,
//! kept as a member of the event (beside its wrapper, on an engine that
//! keeps members so).

use madcawl_js::{Engine, Member};
use madcawl_str::DomString;

use crate::convert::{Any, Thrown};
use crate::host::DomHost;
use crate::side::SideRef;

pub(crate) fn constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, ty: DomString, init: E::Value) -> Result<SideRef, Thrown> {
    let (r, detail) = super::event::make(e, h, ty, init, true)?;
    h.set_detail(e, r, detail.get());
    e.release(detail);
    Ok(r)
}

pub(crate) fn detail<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
    let v = h.details.get(&this).and_then(Member::get).unwrap_or_else(E::null);
    Ok(Any(e.retain(v)))
}

/// The legacy initializer: does nothing while the event is dispatched.
pub(crate) fn init_custom_event<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    this: SideRef,
    ty: DomString,
    bubbles: bool,
    cancelable: bool,
    detail: E::Value,
) -> Result<(), Thrown> {
    if h.event(this).dispatching {
        return Ok(());
    }
    super::event::init_event(e, h, this, ty, bubbles, cancelable)?;
    h.set_detail(e, this, detail);
    Ok(())
}

impl<E: Engine> DomHost<E> {
    /// Make `v` the detail of event `r`; null is kept as no member.
    pub(crate) fn set_detail(&mut self, e: &mut E, r: SideRef, v: E::Value) {
        if v != E::null() {
            return self.set_value(e, r, v);
        }
        if let Some(mut old) = self.details.remove(&r) {
            old.clear(e);
        }
    }

    /// Make `v` the script value side object `r` carries (`details`).
    pub(crate) fn set_value(&mut self, e: &mut E, r: SideRef, v: E::Value) {
        if let Some(mut old) = self.details.remove(&r) {
            old.clear(e);
        }
        let owner = self.wrap_side(e, r);
        let m = Member::of(e, owner.get(), v);
        e.release(owner);
        self.details.insert(r, m);
    }
}
