use madcawl_dom::NodeId;
use madcawl_js::{Engine, Member};
use madcawl_str::Atom;

use crate::convert::Thrown;
use crate::host::{DomHost, Listener};

pub(crate) fn add_event_listener<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    this: NodeId,
    ty: Atom,
    callback: Option<E::Value>,
    capture: bool,
) -> Result<(), Thrown> {
    let Some(cb) = callback else { return Ok(()) };
    let list = h.listeners.entry(this.index()).or_default();
    let same = |l: &Listener<E>| l.ty == ty && l.capture == capture && l.callback.get() == Some(cb);
    if !list.iter().any(same) {
        list.push(Listener { ty, capture, callback: Member::new(e.retain(cb)) });
    }
    Ok(())
}

pub(crate) fn remove_event_listener<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    this: NodeId,
    ty: Atom,
    callback: Option<E::Value>,
    capture: bool,
) -> Result<(), Thrown> {
    let Some(cb) = callback else { return Ok(()) };
    let Some(list) = h.listeners.get_mut(&this.index()) else { return Ok(()) };
    if let Some(i) = list
        .iter()
        .position(|l| l.ty == ty && l.capture == capture && l.callback.get() == Some(cb))
    {
        let mut l = list.remove(i);
        if list.is_empty() {
            h.listeners.remove(&this.index());
        }
        l.callback.clear(e);
    }
    Ok(())
}
