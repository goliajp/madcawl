//! The MessageEvent interface: its data, a member kept where a
//! CustomEvent keeps its detail, and its origin; no last event id,
//! source or ports are made here.

use madcawl_js::{Engine, Member};
use madcawl_str::DomString;

use crate::convert::{Any, Thrown, plain};
use crate::dict::{convert, dictionary};
use crate::event_kinds::Extra;
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

pub(crate) fn constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, ty: DomString, init: E::Value) -> Result<SideRef, Thrown> {
    let (r, _) = super::event::make(e, h, ty, init, false)?;
    let mut origin = DomString::empty();
    if let Some(d) = dictionary(e, init, "the message event's options")? {
        if let Some(data) = crate::dict::member(e, h, d, "data")? {
            h.set_value(e, r, data.get());
            e.release(data);
        }
        origin = convert(e, h, d, "origin", |e, h, v| crate::convert::usv_string(e, h, v))?.map_or_else(DomString::empty, |s| DomString::from_wtf8(s.as_bytes()));
    }
    h.event_mut(r).extra = Extra::Message { origin };
    Ok(r)
}

pub(crate) fn data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
    let v = h.details.get(&this).and_then(Member::get).unwrap_or_else(E::null);
    Ok(Any(e.retain(v)))
}

pub(crate) fn origin<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    match &h.sides.get(this).data {
        SideData::Event(ev) => match &ev.extra {
            Extra::Message { origin } => Ok(crate::scripts::text(origin)),
            _ => unreachable!("a message event"),
        },
        _ => unreachable!("an event's side object"),
    }
}

pub(crate) fn last_event_id<E: Engine>(_: &mut E, _: &mut DomHost<E>, _: SideRef) -> Result<DomString, Thrown> {
    Ok(DomString::empty())
}

pub(crate) fn source<E: Engine>(_: &mut E, _: &mut DomHost<E>, _: SideRef) -> Result<Any<E>, Thrown> {
    Ok(Any(plain::<E>(E::null())))
}

pub(crate) fn ports<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: SideRef) -> Result<Any<E>, Thrown> {
    Ok(Any(crate::convert::array_of(e, h, Vec::new())))
}
