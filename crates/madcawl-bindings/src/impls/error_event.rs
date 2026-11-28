//! The ErrorEvent interface: a message and where it came from, and the
//! error itself, a member kept where a CustomEvent keeps its detail.

use madcawl_js::{Engine, Member};
use madcawl_str::DomString;

use crate::convert::{Any, Thrown};
use crate::dict::{convert, dictionary};
use crate::event_kinds::{ErrorPlace, Extra};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

pub(crate) fn constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, ty: DomString, init: E::Value) -> Result<SideRef, Thrown> {
    let (r, _) = super::event::make(e, h, ty, init, false)?;
    let mut place = ErrorPlace { message: DomString::empty(), filename: DomString::empty(), line: 0, column: 0 };
    if let Some(d) = dictionary(e, init, "the error event's options")? {
        place.column = convert(e, h, d, "colno", |e, h, v| crate::convert::to_uint32(e, h, v))?.unwrap_or(0);
        if let Some(x) = crate::dict::member(e, h, d, "error")? {
            h.set_value(e, r, x.get());
            e.release(x);
        }
        let file = convert(e, h, d, "filename", |e, h, v| crate::convert::usv_string(e, h, v))?;
        place.filename = file.map_or_else(DomString::empty, |s| DomString::from_wtf8(s.as_bytes()));
        place.line = convert(e, h, d, "lineno", |e, h, v| crate::convert::to_uint32(e, h, v))?.unwrap_or(0);
        place.message = convert(e, h, d, "message", |e, h, v| e.to_string(h, v).ok_or(Thrown))?.unwrap_or_else(DomString::empty);
    }
    h.event_mut(r).extra = Extra::Error(Box::new(place));
    Ok(r)
}

fn place<E: Engine>(h: &DomHost<E>, r: SideRef) -> &ErrorPlace {
    match &h.sides.get(r).data {
        SideData::Event(ev) => match &ev.extra {
            Extra::Error(p) => p,
            _ => unreachable!("an error event"),
        },
        _ => unreachable!("an event's side object"),
    }
}

pub(crate) fn message<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    Ok(place(h, this).message.clone())
}

pub(crate) fn filename<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<String, Thrown> {
    Ok(crate::scripts::text(&place(h, this).filename))
}

pub(crate) fn lineno<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<u32, Thrown> {
    Ok(place(h, this).line)
}

pub(crate) fn colno<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<u32, Thrown> {
    Ok(place(h, this).column)
}

pub(crate) fn error<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
    let v = h.details.get(&this).and_then(Member::get).unwrap_or_else(E::undefined);
    Ok(Any(e.retain(v)))
}
