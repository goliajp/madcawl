//! The attributes CSSOM gives CSSStyleDeclaration for each supported
//! property: the camel-cased one (`backgroundColor`), the webkit-cased
//! one of a `-webkit-` name (`webkitBoxSizing`), and the dashed one of a
//! name with a dash (`background-color`). They are accessors on the
//! prototype, made when script first reaches an element's `style`, not
//! with the window: a realm whose script never does makes none of their
//! hundreds of functions (some 660 attributes; about 1.3 ms in a
//! release build when it happens).
//! Until then the prototype has only the interface's own members. The names of one property share its getter
//! and its setter, which find the property by the word their function
//! was made with.

use madcawl_css::style::property_names;
use madcawl_js::{Bound, Ctx, Engine, HostFn, Key, Owned};
use madcawl_str::DomString;

use crate::convert::{arg, dom_string_null_empty, run, this_side};
use crate::generated::CSSStyleDeclarationClass;
use crate::host::DomHost;
use crate::impls::css_style_declaration::set_of;

/// CSSOM's "CSS property to IDL attribute": each letter after a dash
/// upper case, the dashes dropped, and the first letter lower case when
/// `lower_first`.
fn attribute_name(property: &str, lower_first: bool) -> String {
    let mut out = String::with_capacity(property.len());
    let mut upper = false;
    for (k, c) in property.chars().enumerate() {
        if c == '-' {
            upper = !(k == 0 && lower_first);
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// The attribute names of `property`.
fn names(property: &str) -> impl Iterator<Item = String> {
    let webkit = property.starts_with("-webkit-").then(|| attribute_name(property, true));
    let dashed = property.contains('-').then(|| property.to_owned());
    std::iter::once(attribute_name(property, false)).chain(webkit).chain(dashed)
}

fn get<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, k: usize) -> Owned<E::Value> {
    run(e, h, |e, h| {
        let r = this_side(e, h, this, &CSSStyleDeclarationClass::<E>::DEF)?;
        Ok(h.served_value(r, k as u32, |d| {
            let mut out = String::new();
            d.value(property_names()[k], &mut out);
            DomString::from_wtf8(out.as_bytes())
        }))
    })
}

fn set<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: E::Value, v: E::Value, k: usize) -> Owned<E::Value> {
    run(e, h, |e, h| {
        let r = this_side(e, h, this, &CSSStyleDeclarationClass::<E>::DEF)?;
        let v = dom_string_null_empty(e, h, v)?;
        set_of(e, h, r, property_names()[k], &v, &DomString::empty())
    })
}

// SAFETY: called as a host function made for E and DomHost<E>, with the
// property's index as its word
unsafe extern "C" fn get_abi<E: Engine>(ctx: *mut Ctx, this: u64, _: *const u64, _: usize) -> u64 {
    unsafe {
        let k = (*ctx).data as usize;
        let (e, h) = Ctx::parts::<E, DomHost<E>>(ctx);
        E::to_bits(get(e, h, E::from_bits(this), k).into_raw())
    }
}

// SAFETY: as for get_abi; `argc` words are readable at `argv`
unsafe extern "C" fn set_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
    unsafe {
        let k = (*ctx).data as usize;
        let ((e, h), args) = (Ctx::parts::<E, DomHost<E>>(ctx), E::values(argv, argc));
        E::to_bits(set(e, h, E::from_bits(this), arg::<E>(args, 0), k).into_raw())
    }
}

impl<E: Engine> DomHost<E> {
    /// Define the property attributes on the prototype, once.
    pub(crate) fn define_css_properties(&mut self, e: &mut E) {
        if std::mem::replace(&mut self.css_properties, true) {
            return;
        }
        let iface = e.interface_object(self.realm, &CSSStyleDeclarationClass::<E>::DEF);
        let proto = e.get(self, iface.get(), Key::Str(&DomString::from_wtf8(b"prototype")));
        e.release(iface);
        for (k, &p) in property_names().iter().enumerate() {
            let (get, set): (HostFn, HostFn) = (get_abi::<E>, set_abi::<E>);
            // SAFETY: both read their context as E and DomHost<E>
            let (gb, sb) = unsafe { (Bound::<HostFn, E, DomHost<E>>::host_fn(get), Bound::<HostFn, E, DomHost<E>>::host_fn(set)) };
            let g = e.new_function(self.realm, p, 0, gb, k as u64);
            let s = e.new_function(self.realm, p, 1, sb, k as u64);
            // page script may have frozen the prototype or defined the name
            // itself before this: a name it refuses has no attribute
            for n in names(p) {
                let _defined = e.define_accessor(self.realm, proto.get(), &n, g.get(), s.get(), false);
            }
            e.release(g);
            e.release(s);
        }
        e.release(proto);
    }
}

#[cfg(test)]
mod tests {
    use super::{attribute_name, names};

    #[test]
    fn attribute_names_follow_cssom() {
        assert_eq!(attribute_name("background-color", false), "backgroundColor");
        assert_eq!(attribute_name("-webkit-box-sizing", false), "WebkitBoxSizing");
        assert_eq!(attribute_name("-webkit-box-sizing", true), "webkitBoxSizing");
        assert_eq!(names("float").collect::<Vec<_>>(), ["float"]);
        assert_eq!(names("-webkit-order").collect::<Vec<_>>(), ["WebkitOrder", "webkitOrder", "-webkit-order"]);
    }
}
