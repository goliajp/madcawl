//! An element's `style`: the declaration block of its style attribute,
//! one object per element while it lives (`attr.rs`). The block is parsed
//! from the attribute's value and kept with that value; it is parsed
//! again only when the attribute has another value on the next read,
//! however it was set. A change through the block writes the block's
//! serialization to the attribute, through the path every attribute
//! change takes (which marks the element for restyle), and keeps the
//! value it wrote, so the change is not parsed back.

use madcawl_css::style::Declarations;
use madcawl_dom::NodeId;
use madcawl_js::Engine;
use madcawl_str::{DomString, atom};

use crate::convert::Thrown;
use crate::event::Held;
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

pub(crate) struct StyleState {
    /// The element, by a count.
    pub owner: Held,
    /// The attribute value `decls` was parsed from; `None` for no
    /// attribute.
    src: Option<DomString>,
    /// Whether `decls` is what `src` parses to: not before the first
    /// read, nor after a change the attribute did not take.
    parsed: bool,
    decls: Declarations,
    /// The values the property attributes served since `decls` last
    /// changed, by property index: read again, a value is not made again.
    /// At most one for each supported property.
    served: Vec<(u32, DomString)>,
}

impl StyleState {
    pub(crate) fn new(owner: Held) -> StyleState {
        StyleState { owner, src: None, parsed: false, decls: Declarations::new(), served: Vec::new() }
    }

    pub(crate) fn element(&self) -> Option<NodeId> {
        match &self.owner {
            Held::Node(n) => Some(n.id()),
            _ => None,
        }
    }
}

fn text(s: &DomString) -> String {
    let mut b = Vec::new();
    s.write_wtf8(&mut b);
    String::from_utf8_lossy(&b).into_owned()
}

fn state<E: Engine>(h: &mut DomHost<E>, r: SideRef) -> &mut StyleState {
    match &mut h.sides.get_mut(r).data {
        SideData::Style(s) => s,
        _ => unreachable!("a style declaration's side object"),
    }
}

impl<E: Engine> DomHost<E> {
    /// The declarations of `style` object `r`, up to date with its
    /// element's style attribute.
    pub(crate) fn declarations(&mut self, r: SideRef) -> &Declarations {
        let el = match &self.sides.get(r).data {
            SideData::Style(s) => s.element().expect("a style declaration's element"),
            _ => unreachable!("a style declaration's side object"),
        };
        let now = self.dom.attribute(el, atom!("style"));
        let fresh = match &self.sides.get(r).data {
            SideData::Style(s) => s.parsed && s.src.as_ref() == now,
            _ => unreachable!("a style declaration's side object"),
        };
        if !fresh {
            let now = now.cloned();
            let decls = now.as_ref().map_or_else(Declarations::new, |v| Declarations::parse(&text(v)));
            let s = state(self, r);
            s.src = now;
            s.parsed = true;
            s.decls = decls;
            s.served.clear();
        }
        &state(self, r).decls
    }

    /// The value of supported property `k` (`property_names`) of `r`, by
    /// `make` from the declarations when it was not served before.
    pub(crate) fn served_value(&mut self, r: SideRef, k: u32, make: impl FnOnce(&Declarations) -> DomString) -> DomString {
        self.declarations(r);
        let s = state(self, r);
        if let Some((_, v)) = s.served.iter().find(|x| x.0 == k) {
            return v.clone();
        }
        let v = make(&s.decls);
        s.served.push((k, v.clone()));
        v
    }

    /// Change the declarations of `r` by `f`; when it says they changed
    /// (or `always`), the style attribute gets their serialization.
    pub(crate) fn change_declarations(
        &mut self,
        e: &mut E,
        r: SideRef,
        always: bool,
        f: impl FnOnce(&mut Declarations) -> bool,
    ) -> Result<(), Thrown> {
        self.declarations(r);
        let s = state(self, r);
        if !f(&mut s.decls) && !always {
            return Ok(());
        }
        s.served.clear();
        let mut css = String::new();
        s.decls.write_css(&mut css);
        let el = s.element().expect("a style declaration's element");
        let set = crate::impls::attributes::set(e, self, el, atom!("style"), &DomString::from_wtf8(css.as_bytes()));
        if set.is_err() {
            // the attribute did not take the changed declarations: they
            // are parsed from it again on the next read
            state(self, r).parsed = false;
            return set;
        }
        let now = self.dom.attribute(el, atom!("style")).cloned();
        state(self, r).src = now;
        Ok(())
    }
}
