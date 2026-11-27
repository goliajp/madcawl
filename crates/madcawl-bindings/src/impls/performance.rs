//! The window's Performance: the time since the time origin, coarsened
//! to 100 microseconds as the High Resolution Time Standard has it for
//! a context that is not cross-origin isolated, and the Performance
//! Timeline's entry buffer, empty while no entry type is supported.

use madcawl_js::Engine;
use madcawl_str::DomString;

use crate::convert::{Any, Thrown, array_of};
use crate::host::DomHost;
use crate::side::SideRef;

/// Milliseconds `ms` floored to 100 microseconds.
pub(crate) fn coarsen(ms: f64) -> f64 {
    (ms * 10.0).floor() / 10.0
}

pub(crate) fn now<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<f64, Thrown> {
    Ok(coarsen(h.now_ms()))
}

pub(crate) fn time_origin<E: Engine>(_: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<f64, Thrown> {
    Ok(coarsen(h.time_origin_ms))
}

fn none<E: Engine>(e: &mut E, h: &mut DomHost<E>) -> Result<Any<E>, Thrown> {
    Ok(Any(array_of(e, h, Vec::new())))
}

pub(crate) fn get_entries<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<Any<E>, Thrown> {
    none(e, h)
}

pub(crate) fn get_entries_by_type<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef, _ty: DomString) -> Result<Any<E>, Thrown> {
    none(e, h)
}

pub(crate) fn get_entries_by_name<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    _: SideRef,
    _name: DomString,
    _ty: Option<DomString>,
) -> Result<Any<E>, Thrown> {
    none(e, h)
}

#[cfg(test)]
mod tests {
    #[test]
    fn time_is_floored_to_a_tenth_of_a_millisecond() {
        assert_eq!(super::coarsen(12.3456), 12.3);
        assert_eq!(super::coarsen(0.09), 0.0);
        assert_eq!(super::coarsen(1_800_000_000_123.47), 1_800_000_000_123.4);
    }
}
