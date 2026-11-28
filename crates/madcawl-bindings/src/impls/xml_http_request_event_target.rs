//! The XMLHttpRequestEventTarget interface: the event handlers of an
//! XMLHttpRequest's progress events.

use madcawl_js::Engine;

use crate::convert::{Any, Thrown};
use crate::event::Target;
use crate::host::DomHost;
use crate::side::SideRef;

macro_rules! handlers {
    ($($get:ident, $set:ident => $ty:literal;)*) => {$(
        pub(crate) fn $get<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Any<E>, Thrown> {
            let ty = h.names.hold($ty);
            Ok(Any(h.event_handler(e, Target::Side(this), ty)))
        }

        pub(crate) fn $set<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, value: Option<E::Value>) -> Result<(), Thrown> {
            let ty = h.names.hold($ty);
            h.set_event_handler(e, Target::Side(this), ty, value);
            Ok(())
        }
    )*};
}

handlers! {
    onloadstart, set_onloadstart => b"loadstart";
    onprogress, set_onprogress => b"progress";
    onabort, set_onabort => b"abort";
    onerror, set_onerror => b"error";
    onload, set_onload => b"load";
    ontimeout, set_ontimeout => b"timeout";
    onloadend, set_onloadend => b"loadend";
}
