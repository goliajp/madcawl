//! The HTMLElement interface's members: reflected attributes, `hidden`
//! with its until-found state, the element's nonce, and three event
//! handlers.

use madcawl_dom::NodeId;
use madcawl_html::AttrNamespace;
use madcawl_js::{Engine, ValueKind};
use madcawl_str::{Atom, DomString, atom};

use super::attributes::{set, take_off};
use super::reflect::{keyword, string};
use crate::convert::{Any, Thrown, dom_string};
use crate::event::Target;
use crate::host::DomHost;

pub(crate) fn title<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(string(h, this, atom!("title")))
}

pub(crate) fn set_title<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, v: DomString) -> Result<(), Thrown> {
    set(e, h, this, atom!("title"), &v)
}

pub(crate) fn lang<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(string(h, this, atom!("lang")))
}

pub(crate) fn set_lang<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, v: DomString) -> Result<(), Thrown> {
    set(e, h, this, atom!("lang"), &v)
}

pub(crate) fn dir<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(keyword(h, this, atom!("dir"), &["ltr", "rtl", "auto"]))
}

pub(crate) fn set_dir<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, v: DomString) -> Result<(), Thrown> {
    set(e, h, this, atom!("dir"), &v)
}

fn is_until_found(v: &DomString) -> bool {
    let mut b = Vec::new();
    v.write_wtf8(&mut b);
    b.eq_ignore_ascii_case(b"until-found")
}

/// "until-found" in the hidden until found state, otherwise whether the
/// attribute is there.
pub(crate) fn hidden<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Any<E>, Thrown> {
    Ok(Any(match h.dom.attribute(this, atom!("hidden")) {
        Some(v) if is_until_found(v) => e.string(DomString::from_wtf8(b"until-found")),
        // SAFETY: a boolean carries no count
        v => unsafe { madcawl_js::Owned::from_raw(E::boolean(v.is_some())) },
    }))
}

/// The value converted as the standard's union: null, false, 0, NaN and
/// the empty string remove the attribute, "until-found" in any case
/// sets it so, anything else sets it empty.
pub(crate) fn set_hidden<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, v: E::Value) -> Result<(), Thrown> {
    let keep = match e.kind(v) {
        ValueKind::Undefined | ValueKind::Null => None,
        ValueKind::Boolean => e.as_boolean(v).filter(|&b| b).map(|_| DomString::empty()),
        ValueKind::Number => e.as_number(v).filter(|n| *n != 0.0 && !n.is_nan()).map(|_| DomString::empty()),
        _ => {
            let s = dom_string(e, h, v)?;
            if is_until_found(&s) {
                Some(DomString::from_wtf8(b"until-found"))
            } else {
                (!s.is_empty()).then(DomString::empty)
            }
        }
    };
    match keep {
        Some(value) => set(e, h, this, atom!("hidden"), &value),
        None => {
            let k = h.dom.attributes(this).position(|a| a.ns == AttrNamespace::None && a.local == atom!("hidden"));
            if let Some(k) = k {
                take_off(e, h, this, k);
            }
            Ok(())
        }
    }
}

/// The element's cryptographic nonce: what script set, until the
/// content attribute changes, otherwise the attribute's value.
pub(crate) fn nonce<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(match h.nonces.get(&this.index()) {
        Some(v) => v.clone(),
        None => string(h, this, atom!("nonce")),
    })
}

/// Sets the nonce, not the content attribute.
pub(crate) fn set_nonce<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId, v: DomString) -> Result<(), Thrown> {
    h.nonces.insert(this.index(), v);
    Ok(())
}

/// Attribute `local` of namespace `ns` of `el` was set or removed: a
/// nonce attribute sets the nonce again, the attribute's value (or the
/// empty string) being what it reads.
pub(crate) fn attribute_changed<E: Engine>(h: &mut DomHost<E>, el: NodeId, ns: AttrNamespace, local: Atom) {
    if local == atom!("nonce") && ns == AttrNamespace::None && !h.nonces.is_empty() {
        h.nonces.remove(&el.index());
    }
}

/// As `attribute_changed`, for the attribute at `k`.
pub(crate) fn attribute_changed_at<E: Engine>(h: &mut DomHost<E>, el: NodeId, k: usize) {
    let a = h.dom.attributes(el).nth(k).expect("the attribute");
    let (ns, local) = (a.ns, a.local);
    attribute_changed(h, el, ns, local);
}

fn handler<E: Engine>(e: &mut E, h: &mut DomHost<E>, t: Target, ty: Atom) -> Result<Any<E>, Thrown> {
    Ok(Any(h.event_handler(e, t, ty)))
}

fn set_handler<E: Engine>(e: &mut E, h: &mut DomHost<E>, t: Target, ty: Atom, v: Option<E::Value>) -> Result<(), Thrown> {
    h.set_event_handler(e, t, ty, v);
    Ok(())
}

pub(crate) fn onclick<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Any<E>, Thrown> {
    handler(e, h, Target::Node(this), atom!("click"))
}

pub(crate) fn set_onclick<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, v: Option<E::Value>) -> Result<(), Thrown> {
    set_handler(e, h, Target::Node(this), atom!("click"), v)
}

pub(crate) fn onerror<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Any<E>, Thrown> {
    handler(e, h, Target::Node(this), atom!("error"))
}

pub(crate) fn set_onerror<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, v: Option<E::Value>) -> Result<(), Thrown> {
    set_handler(e, h, Target::Node(this), atom!("error"), v)
}

pub(crate) fn onload<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<Any<E>, Thrown> {
    handler(e, h, Target::Node(this), atom!("load"))
}

pub(crate) fn set_onload<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, v: Option<E::Value>) -> Result<(), Thrown> {
    set_handler(e, h, Target::Node(this), atom!("load"), v)
}

/// The body element's `onerror` and `onload`: the window's.
pub(crate) mod body {
    use super::*;

    pub(crate) fn onerror<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: NodeId) -> Result<Any<E>, Thrown> {
        handler(e, h, Target::Window, atom!("error"))
    }

    pub(crate) fn set_onerror<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: NodeId, v: Option<E::Value>) -> Result<(), Thrown> {
        set_handler(e, h, Target::Window, atom!("error"), v)
    }

    pub(crate) fn onload<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: NodeId) -> Result<Any<E>, Thrown> {
        handler(e, h, Target::Window, atom!("load"))
    }

    pub(crate) fn set_onload<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: NodeId, v: Option<E::Value>) -> Result<(), Thrown> {
        set_handler(e, h, Target::Window, atom!("load"), v)
    }
}
