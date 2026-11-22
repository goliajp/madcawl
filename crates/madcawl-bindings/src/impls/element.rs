use madcawl_dom::{Dom, NodeId};
use madcawl_js::Engine;
use madcawl_str::{Atom, DomString};

use crate::convert::Thrown;
use crate::host::DomHost;

/// An HTML element's name in ASCII upper case; others' as they are.
pub(crate) fn html_upper(dom: &Dom, el: NodeId) -> DomString {
    let mut b = Vec::new();
    dom.local_name(el).expect("an element").write_wtf8(&mut b);
    if dom.in_html_namespace(el) {
        b.make_ascii_uppercase();
    }
    DomString::from_wtf8(&b)
}

fn atom_string(a: Option<Atom>) -> DomString {
    a.map_or_else(DomString::empty, Atom::to_dom_string)
}

pub(crate) fn local_name<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(atom_string(h.dom.local_name(this)))
}

pub(crate) fn tag_name<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(html_upper(&h.dom, this))
}

pub(crate) fn id<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(atom_string(h.dom.element_id(this)))
}

pub(crate) fn class_name<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    get_attribute(e, h, this, DomString::from_wtf8(b"class")).map(|v| v.unwrap_or_else(DomString::empty))
}

/// The value of the first attribute whose qualified name is `name`,
/// lowercased first on an HTML element.
pub(crate) fn get_attribute<E: Engine>(
    _: &mut E,
    h: &mut DomHost<E>,
    this: NodeId,
    name: DomString,
) -> Result<Option<DomString>, Thrown> {
    let dom = &h.dom;
    let mut want = Vec::new();
    name.write_wtf8(&mut want);
    if dom.in_html_namespace(this) {
        want.make_ascii_lowercase();
    }
    let mut q = Vec::new();
    Ok(dom
        .attributes(this)
        .find(|a| {
            q.clear();
            if let Some(p) = a.prefix {
                p.write_wtf8(&mut q);
                q.push(b':');
            }
            a.local.write_wtf8(&mut q);
            q == want
        })
        .map(|a| a.value.clone()))
}

pub(crate) fn has_attribute<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, name: DomString) -> Result<bool, Thrown> {
    get_attribute(e, h, this, name).map(|v| v.is_some())
}
