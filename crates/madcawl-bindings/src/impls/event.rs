//! The Event interface's members.

use madcawl_js::{Engine, Key, Owned, ValueKind};
use madcawl_str::DomString;

use crate::convert::{Thrown, throw};
use crate::event::{EventState, Held, Target};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

/// `new Event(type, eventInitDict)`: the dictionary's members are read
/// in the order Web IDL converts a dictionary, by name.
pub(crate) fn constructor<E: Engine>(e: &mut E, h: &mut DomHost<E>, ty: DomString, init: E::Value) -> Result<SideRef, Thrown> {
    let (r, _) = make(e, h, ty, init, false)?;
    Ok(r)
}

/// A new event of type `ty` from dictionary `init`; with `custom`, a
/// CustomEvent, and its `detail` member (null when absent).
pub(crate) fn make<E: Engine>(e: &mut E, h: &mut DomHost<E>, ty: DomString, init: E::Value, custom: bool) -> Result<(SideRef, Owned<E::Value>), Thrown> {
    let mut flags = [false; 3];
    // SAFETY: null carries no count
    let mut detail = unsafe { Owned::from_raw(E::null()) };
    match e.kind(init) {
        ValueKind::Undefined | ValueKind::Null => {}
        ValueKind::Object | ValueKind::Function => {
            let names: &[&str] = if custom { &["bubbles", "cancelable", "composed", "detail"] } else { &["bubbles", "cancelable", "composed"] };
            for (i, name) in names.iter().enumerate() {
                let v = e.get(h, init, Key::Str(&DomString::from_wtf8(name.as_bytes())));
                if E::is_thrown(v.get()) {
                    let _sentinel = v.into_raw();
                    e.release(detail);
                    return Err(Thrown);
                }
                if i == 3 {
                    if e.kind(v.get()) != ValueKind::Undefined {
                        detail = v;
                        continue;
                    }
                } else {
                    flags[i] = e.to_boolean(v.get());
                }
                e.release(v);
            }
        }
        _ => return Err(throw(e, madcawl_js::ErrorKind::Type, "the event's options are not a dictionary")),
    }
    let mut ev = EventState::new(h.names.hold(&wtf8(&ty)), h.now_ms());
    [ev.bubbles, ev.cancelable, ev.composed] = flags;
    ev.custom = custom;
    Ok((h.sides.add(SideData::Event(ev)), detail))
}

pub(crate) fn wtf8(s: &DomString) -> Vec<u8> {
    let mut b = Vec::new();
    s.write_wtf8(&mut b);
    b
}

pub(crate) fn r#type<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    Ok(h.event(this).ty.to_dom_string())
}

fn held(h: &Option<Held>) -> Option<Target> {
    h.as_ref().map(Held::target)
}

pub(crate) fn target<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Option<Target>, Thrown> {
    Ok(held(&h.event(this).target))
}

pub(crate) fn src_element<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Option<Target>, Thrown> {
    target(e, h, this)
}

pub(crate) fn current_target<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<Option<Target>, Thrown> {
    Ok(held(&h.event(this).current))
}

pub(crate) fn composed_path<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<TargetList, Thrown> {
    Ok(TargetList(h.event(this).path.iter().map(Held::target).collect()))
}

/// The targets `composedPath` returns, as an array.
pub(crate) struct TargetList(Vec<Target>);

impl<E: Engine> crate::convert::IntoJs<E> for TargetList {
    fn into_js(self, e: &mut E, h: &mut DomHost<E>) -> madcawl_js::Owned<E::Value> {
        let items = self.0.into_iter().map(|t| t.into_js(e, h)).collect();
        crate::convert::array_of(e, h, items)
    }
}

pub(crate) fn event_phase<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<u16, Thrown> {
    Ok(h.event(this).phase)
}

pub(crate) fn stop_propagation<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<(), Thrown> {
    h.event_mut(this).stop = true;
    Ok(())
}

pub(crate) fn cancel_bubble<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<bool, Thrown> {
    Ok(h.event(this).stop)
}

pub(crate) fn set_cancel_bubble<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, value: bool) -> Result<(), Thrown> {
    if value {
        h.event_mut(this).stop = true;
    }
    Ok(())
}

pub(crate) fn stop_immediate_propagation<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<(), Thrown> {
    let ev = h.event_mut(this);
    (ev.stop, ev.stop_now) = (true, true);
    Ok(())
}

pub(crate) fn bubbles<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<bool, Thrown> {
    Ok(h.event(this).bubbles)
}

pub(crate) fn cancelable<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<bool, Thrown> {
    Ok(h.event(this).cancelable)
}

pub(crate) fn return_value<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<bool, Thrown> {
    Ok(!h.event(this).canceled)
}

pub(crate) fn set_return_value<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, value: bool) -> Result<(), Thrown> {
    if !value {
        h.event_mut(this).cancel();
    }
    Ok(())
}

pub(crate) fn prevent_default<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<(), Thrown> {
    h.event_mut(this).cancel();
    Ok(())
}

pub(crate) fn default_prevented<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<bool, Thrown> {
    Ok(h.event(this).canceled)
}

pub(crate) fn composed<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<bool, Thrown> {
    Ok(h.event(this).composed)
}

pub(crate) fn is_trusted<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<bool, Thrown> {
    Ok(h.event(this).trusted)
}

pub(crate) fn time_stamp<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<f64, Thrown> {
    Ok(h.event(this).time)
}

/// The legacy initializer: does nothing while the event is dispatched.
pub(crate) fn init_event<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, ty: DomString, bubbles: bool, cancelable: bool) -> Result<(), Thrown> {
    if h.event(this).dispatching {
        return Ok(());
    }
    let ty = h.names.hold(&wtf8(&ty));
    let ev = h.event_mut(this);
    (ev.ty, ev.bubbles, ev.cancelable, ev.initialized) = (ty, bubbles, cancelable, true);
    (ev.stop, ev.stop_now, ev.canceled, ev.trusted) = (false, false, false, false);
    h.set_target(e, this, None);
    Ok(())
}
