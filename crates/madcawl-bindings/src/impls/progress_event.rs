//! The ProgressEvent interface: an event with two counts and whether the
//! second is known.

use madcawl_js::Engine;
use madcawl_str::DomString;

use crate::convert::Thrown;
use crate::dict::{convert, dictionary};
use crate::event_kinds::Extra;
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

/// An `unsigned long long` as Web IDL converts one, modulo 2^64, as a
/// double.
fn to_u64(n: f64) -> f64 {
    if !n.is_finite() {
        return 0.0;
    }
    let n = n.trunc() % 2f64.powi(64);
    if n < 0.0 { n + 2f64.powi(64) } else { n }
}

pub(crate) fn constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, ty: DomString, init: E::Value) -> Result<SideRef, Thrown> {
    let (r, _) = super::event::make(e, h, ty, init, false)?;
    let mut extra = Extra::Progress { computable: false, loaded: 0.0, total: 0.0 };
    if let Some(d) = dictionary(e, init, "the progress event's options")? {
        let computable = convert(e, h, d, "lengthComputable", |e, _, v| Ok(e.to_boolean(v)))?;
        let loaded = convert(e, h, d, "loaded", |e, h, v| e.to_number(h, v).ok_or(Thrown))?;
        let total = convert(e, h, d, "total", |e, h, v| e.to_number(h, v).ok_or(Thrown))?;
        extra = Extra::Progress { computable: computable.unwrap_or(false), loaded: to_u64(loaded.unwrap_or(0.0)), total: to_u64(total.unwrap_or(0.0)) };
    }
    h.event_mut(r).extra = extra;
    Ok(r)
}

fn counts<E: Engine>(h: &DomHost<E>, r: SideRef) -> (bool, f64, f64) {
    match &h.sides.get(r).data {
        SideData::Event(ev) => match ev.extra {
            Extra::Progress { computable, loaded, total } => (computable, loaded, total),
            _ => unreachable!("a progress event"),
        },
        _ => unreachable!("an event's side object"),
    }
}

pub(crate) fn length_computable<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<bool, Thrown> {
    Ok(counts(h, this).0)
}

pub(crate) fn loaded<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<f64, Thrown> {
    Ok(counts(h, this).1)
}

pub(crate) fn total<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<f64, Thrown> {
    Ok(counts(h, this).2)
}

impl<E: Engine> DomHost<E> {
    /// Fire a trusted ProgressEvent of type `ty` at side object `target`,
    /// which cannot be canceled and does not bubble.
    pub(crate) fn fire_progress(&mut self, e: &mut E, target: SideRef, ty: madcawl_str::Atom, loaded: u64, total: Option<u64>) {
        let mut ev = crate::event::EventState::new(ty, self.now_ms());
        ev.trusted = true;
        ev.extra = Extra::Progress { computable: total.is_some(), loaded: loaded as f64, total: total.unwrap_or(0) as f64 };
        self.fire_built(e, ev, crate::event::Target::Side(target));
    }

    /// Dispatch event `ev`, made by the host, at `target`, with a
    /// checkpoint after it when no script is running, as `fire_simple`
    /// does; whether no listener canceled it.
    pub(crate) fn fire_built(&mut self, e: &mut E, ev: crate::event::EventState, target: crate::event::Target) -> bool {
        let r = self.sides.add(SideData::Event(ev));
        self.fire_side(e, r, target)
    }

    /// [`DomHost::fire_built`] for an event already made, side object `r`.
    pub(crate) fn fire_side(&mut self, e: &mut E, r: SideRef, target: crate::event::Target) -> bool {
        let Ok(not_canceled) = self.dispatch(e, r, target) else { unreachable!("a new event is not being dispatched") };
        self.recover(e);
        if !e.in_script() && e.halt() == madcawl_js::Halt::Running {
            self.checkpoint(e);
        }
        self.settle(e);
        not_canceled
    }
}
