use madcawl_dom::NodeId;
use madcawl_js::{Engine, Member};
use madcawl_str::DomString;

use crate::convert::Thrown;
use crate::host::{DomHost, Listener};

fn atom_of<E: Engine>(h: &DomHost<E>, ty: &DomString) -> madcawl_str::Atom {
    let mut b = Vec::new();
    ty.write_wtf8(&mut b);
    h.names.hold(&b)
}

pub(crate) fn add_event_listener<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    this: NodeId,
    ty: DomString,
    callback: Option<E::Value>,
    capture: bool,
) -> Result<(), Thrown> {
    let Some(cb) = callback else { return Ok(()) };
    let ty = atom_of(h, &ty);
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
    ty: DomString,
    callback: Option<E::Value>,
    capture: bool,
) -> Result<(), Thrown> {
    let Some(cb) = callback else { return Ok(()) };
    let ty = atom_of(h, &ty);
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
