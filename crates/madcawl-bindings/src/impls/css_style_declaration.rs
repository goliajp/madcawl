//! The CSSStyleDeclaration interface, as an element's `style`
//! (`style_attr.rs`). A property name is a custom property's as given and
//! any other's ASCII-lowercased; a property this engine does not know is
//! not supported: setting it does nothing and reading it gives the empty
//! string.

use madcawl_css::style::{Declarations, canonical};
use madcawl_js::Engine;
use madcawl_str::DomString;

use crate::convert::Thrown;
use crate::host::DomHost;
use crate::side::SideRef;

fn name(s: &DomString) -> String {
    let mut b = Vec::new();
    s.write_wtf8(&mut b);
    let mut n = String::from_utf8_lossy(&b).into_owned();
    if !n.starts_with("--") {
        n.make_ascii_lowercase();
    }
    n
}

fn text(s: &DomString) -> String {
    let mut b = Vec::new();
    s.write_wtf8(&mut b);
    String::from_utf8_lossy(&b).into_owned()
}

fn string(s: &str) -> DomString {
    DomString::from_wtf8(s.as_bytes())
}

/// `getPropertyValue` of a property name as `name` gives it.
pub(crate) fn value_of<E: Engine>(h: &mut DomHost<E>, this: SideRef, property: &str) -> DomString {
    let mut out = String::new();
    h.declarations(this).value(property, &mut out);
    string(&out)
}

/// `setProperty` of a property name as `name` gives it.
pub(crate) fn set_of<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, property: &str, value: &DomString, priority: &DomString) -> Result<(), Thrown> {
    if !property.starts_with("--") && canonical(property).is_none() {
        return Ok(());
    }
    if value.is_empty() {
        return h.change_declarations(e, this, false, |d| d.remove(property));
    }
    let important = match text(priority) {
        p if p.is_empty() => false,
        p if p.eq_ignore_ascii_case("important") => true,
        _ => return Ok(()),
    };
    let value = text(value);
    h.change_declarations(e, this, false, |d| d.set(property, &value, important))
}

pub(crate) fn css_text<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    let mut out = String::new();
    h.declarations(this).write_css(&mut out);
    Ok(string(&out))
}

pub(crate) fn set_css_text<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, v: DomString) -> Result<(), Thrown> {
    let parsed = Declarations::parse(&text(&v));
    h.change_declarations(e, this, true, |d| {
        *d = parsed;
        true
    })
}

pub(crate) fn length<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<u32, Thrown> {
    Ok(h.declarations(this).len() as u32)
}

pub(crate) fn item<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, i: u32) -> Result<Option<DomString>, Thrown> {
    Ok(h.declarations(this).name(i as usize).map(string))
}

pub(crate) fn get_property_value<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, property: DomString) -> Result<DomString, Thrown> {
    Ok(value_of(h, this, &name(&property)))
}

pub(crate) fn get_property_priority<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef, property: DomString) -> Result<DomString, Thrown> {
    let important = h.declarations(this).important(&name(&property));
    Ok(string(if important { "important" } else { "" }))
}

pub(crate) fn set_property<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, property: DomString, value: DomString, priority: DomString) -> Result<(), Thrown> {
    set_of(e, h, this, &name(&property), &value, &priority)
}

pub(crate) fn remove_property<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, property: DomString) -> Result<DomString, Thrown> {
    let property = name(&property);
    let old = value_of(h, this, &property);
    h.change_declarations(e, this, false, |d| d.remove(&property))?;
    Ok(old)
}

/// No rule is bound: null.
pub(crate) fn parent_rule<E: Engine>(_: &mut E, _: &mut DomHost<E>, _: SideRef) -> Result<Option<DomString>, Thrown> {
    Ok(None)
}

pub(crate) fn css_float<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: SideRef) -> Result<DomString, Thrown> {
    Ok(value_of(h, this, "float"))
}

pub(crate) fn set_css_float<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: SideRef, v: DomString) -> Result<(), Thrown> {
    set_of(e, h, this, "float", &v, &DomString::empty())
}
