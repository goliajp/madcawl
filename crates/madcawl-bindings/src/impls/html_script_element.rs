//! The HTMLScriptElement interface's members: its attributes reflected,
//! and `async` with the element's force async flag. Setting `src` or
//! `text` on a connected script element may run it (`scripts.rs`).

use madcawl_dom::NodeId;
use madcawl_js::Engine;
use madcawl_str::{Atom, DomString, atom};

use super::attributes::set;
use crate::convert::Thrown;
use crate::host::DomHost;

fn toggle<E: Engine>(e: &mut E, h: &mut DomHost<E>, el: NodeId, name: Atom, on: bool) -> Result<(), Thrown> {
    if on {
        return set(e, h, el, name, &DomString::empty());
    }
    let k = h.dom.attributes(el).position(|a| a.ns == madcawl_html::AttrNamespace::None && a.local == name);
    if let Some(k) = k {
        super::attributes::take_off(e, h, el, k);
    }
    Ok(())
}

pub(crate) fn src<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(super::reflect::url(h, this, atom!("src")))
}

pub(crate) fn set_src<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, value: DomString) -> Result<(), Thrown> {
    set(e, h, this, atom!("src"), &value)
}

pub(crate) fn r#type<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(super::reflect::string(h, this, atom!("type")))
}

pub(crate) fn set_type<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, value: DomString) -> Result<(), Thrown> {
    set(e, h, this, atom!("type"), &value)
}

pub(crate) fn r#async<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<bool, Thrown> {
    Ok(h.scripts.flags(this).force_async || h.dom.attribute(this, atom!("async")).is_some())
}

pub(crate) fn set_async<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, value: bool) -> Result<(), Thrown> {
    let f = crate::scripts::Flags { force_async: false, ..h.scripts.flags(this) };
    h.scripts.flags.insert(this.index(), f);
    toggle(e, h, this, atom!("async"), value)
}

pub(crate) fn defer<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<bool, Thrown> {
    Ok(h.dom.attribute(this, atom!("defer")).is_some())
}

pub(crate) fn set_defer<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, value: bool) -> Result<(), Thrown> {
    toggle(e, h, this, atom!("defer"), value)
}

pub(crate) fn text<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(h.child_text(this))
}

pub(crate) fn set_text<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, value: DomString) -> Result<(), Thrown> {
    super::node::set_text_content(e, h, this, Some(value))
}
