use madcawl_dom::NodeId;
use madcawl_js::{Engine, Key, Member, ValueKind};
use madcawl_str::{Atom, DomString};

use crate::convert::Thrown;
use crate::host::{DomHost, Listener};
use crate::side::SideRef;

/// The third argument: a boolean (capture), or an options dictionary
/// whose members are read by name; `(capture, once)`.
fn options<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value, add: bool) -> Result<(bool, bool), Thrown> {
    if !matches!(e.kind(v), ValueKind::Object | ValueKind::Function) {
        return Ok((e.to_boolean(v), false));
    }
    let names: &[&str] = if add { &["capture", "once"] } else { &["capture"] };
    let mut out = [false; 2];
    for (name, slot) in names.iter().zip(&mut out) {
        let x = e.get(h, v, Key::Str(&DomString::from_wtf8(name.as_bytes())));
        if E::is_thrown(x.get()) {
            let _sentinel = x.into_raw();
            return Err(Thrown);
        }
        *slot = e.to_boolean(x.get());
        e.release(x);
    }
    Ok((out[0], out[1]))
}

pub(crate) fn add_event_listener<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    this: NodeId,
    ty: Atom,
    callback: Option<E::Value>,
    opts: E::Value,
) -> Result<(), Thrown> {
    let (capture, once) = options(e, h, opts, true)?;
    let Some(cb) = callback else { return Ok(()) };
    let list = h.listeners.entry(this.index()).or_default();
    let same = |l: &Listener<E>| l.ty == ty && l.capture == capture && l.callback.get() == Some(cb);
    if !list.iter().any(same) {
        list.push(Listener { ty, capture, once, callback: Member::new(e.retain(cb)) });
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
    let (capture, _) = options(e, h, opts, false)?;
    if let Some(cb) = callback {
        h.remove_listener(e, this, ty, capture, cb);
    }
    Ok(())
}

pub(crate) fn dispatch_event<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, event: SideRef) -> Result<bool, Thrown> {
    h.dispatch(e, event, this)
}

impl<E: Engine> DomHost<E> {
    pub(crate) fn remove_listener(&mut self, e: &mut E, n: NodeId, ty: Atom, capture: bool, cb: E::Value) {
        let Some(list) = self.listeners.get_mut(&n.index()) else { return };
        if let Some(i) = list.iter().position(|l| l.ty == ty && l.capture == capture && l.callback.get() == Some(cb)) {
            let mut l = list.remove(i);
            if list.is_empty() {
                self.listeners.remove(&n.index());
            }
            l.callback.clear(e);
        }
    }
}
