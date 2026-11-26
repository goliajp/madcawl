use madcawl_dom::NodeId;
use madcawl_js::Engine;
use madcawl_str::Atom;

use crate::convert::Thrown;
use crate::event::Target;
use crate::host::DomHost;
use crate::listeners::options;
use crate::side::SideRef;

pub(crate) fn add_event_listener<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    this: NodeId,
    ty: Atom,
    callback: Option<E::Value>,
    opts: E::Value,
) -> Result<(), Thrown> {
    let o = options(e, h, opts, true)?;
    if let Some(cb) = callback {
        h.add_listener(e, Target::Node(this), ty, cb, &o);
    }
    Ok(())
}

pub(crate) fn remove_event_listener<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    this: NodeId,
    ty: Atom,
    callback: Option<E::Value>,
    opts: E::Value,
) -> Result<(), Thrown> {
    let o = options(e, h, opts, false)?;
    if let Some(cb) = callback {
        h.remove_listener(e, Target::Node(this), ty, o.capture, cb);
    }
    Ok(())
}

pub(crate) fn dispatch_event<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, event: SideRef) -> Result<bool, Thrown> {
    h.dispatch(e, event, this)
}
