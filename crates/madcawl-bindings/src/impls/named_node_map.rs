//! The NamedNodeMap interface: an element's attributes, in their order,
//! as `Attr`s.

use madcawl_dom::NodeId;
use madcawl_js::Engine;
use madcawl_str::DomString;

use super::attributes::{find, find_ns, set_node, take_off};
use crate::convert::{Thrown, throw_dom};
use crate::host::DomHost;
use crate::side::SideRef;

fn element<E: Engine>(h: &DomHost<E>, this: SideRef) -> NodeId {
    h.owning_element(this).expect("a map's element")
}

pub(crate) fn length<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<u32, Thrown> {
    Ok(h.dom.attributes(element(h, this)).count() as u32)
}

pub(crate) fn item<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, i: u32) -> Result<Option<SideRef>, Thrown> {
    let el = element(h, this);
    let n = h.dom.attributes(el).count();
    Ok(((i as usize) < n).then(|| h.attr_of(el, i as usize)))
}

pub(crate) fn get_named_item<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, name: DomString) -> Result<Option<SideRef>, Thrown> {
    let el = element(h, this);
    Ok(find(h, el, &name).map(|k| h.attr_of(el, k)))
}

pub(crate) fn get_named_item_ns<E: Engine>(
    _: &mut E,
    h: &mut DomHost<E>,
    this: SideRef,
    ns: Option<DomString>,
    local: DomString,
) -> Result<Option<SideRef>, Thrown> {
    let el = element(h, this);
    Ok(find_ns(h, el, ns, &local).map(|k| h.attr_of(el, k)))
}

pub(crate) fn set_named_item<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, attr: SideRef) -> Result<Option<SideRef>, Thrown> {
    let el = element(h, this);
    set_node(e, h, el, attr)
}

pub(crate) use set_named_item as set_named_item_ns;

/// Take attribute `k` off the map's element, as its `Attr`; a
/// NotFoundError when there is none.
fn remove<E: Engine>(e: &mut E, h: &mut DomHost<E>, el: NodeId, k: Option<usize>) -> Result<SideRef, Thrown> {
    let Some(k) = k else { return Err(throw_dom(e, h, "NotFoundError", "the element has no such attribute")) };
    let r = h.attr_of(el, k);
    take_off(e, h, el, k);
    Ok(r)
}

pub(crate) fn remove_named_item<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, name: DomString) -> Result<SideRef, Thrown> {
    let el = element(h, this);
    let k = find(h, el, &name);
    remove(e, h, el, k)
}

pub(crate) fn remove_named_item_ns<E: Engine>(
    e: &mut E,
    h: &mut DomHost<E>,
    this: SideRef,
    ns: Option<DomString>,
    local: DomString,
) -> Result<SideRef, Thrown> {
    let el = element(h, this);
    let k = find_ns(h, el, ns, &local);
    remove(e, h, el, k)
}
