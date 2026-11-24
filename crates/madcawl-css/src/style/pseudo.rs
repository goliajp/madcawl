//! The pseudo-classes, for a page no one has interacted with: nothing
//! is hovered, focused, visited or targeted, form controls hold the
//! values their markup gives them, and custom elements are undefined.

use madcawl_dom::{NodeId, NodeKind};
use madcawl_str::{Atom, atom};

use crate::selector::{Dir, PseudoClass};

use super::matching::{Matcher, text};

/// The input types whose value is text a person types.
const TEXT_TYPES: &[&str] = &[
    "",
    "date",
    "datetime-local",
    "email",
    "month",
    "number",
    "password",
    "search",
    "tel",
    "text",
    "time",
    "url",
    "week",
];

impl Matcher<'_, '_> {
    fn html(&self, el: NodeId, name: Atom) -> bool {
        self.is_html(el) && self.dom.local_name(el) == Some(name)
    }

    fn has_attr(&self, el: NodeId, name: Atom) -> bool {
        self.dom.attribute(el, name).is_some()
    }

    fn input_type(&self, el: NodeId) -> String {
        self.dom
            .attribute(el, atom!("type"))
            .map(|t| text(t).trim().to_ascii_lowercase())
            .unwrap_or_default()
    }

    fn checkable(&self, el: NodeId) -> bool {
        self.html(el, atom!("input")) && matches!(&*self.input_type(el), "checkbox" | "radio")
    }

    pub(crate) fn pseudo(&self, p: PseudoClass, el: NodeId) -> bool {
        use PseudoClass as P;
        match p {
            P::Root | P::Scope => self.is_root(el),
            P::Empty => self.dom.children(el).all(|c| match self.dom.kind(c) {
                NodeKind::Element => false,
                NodeKind::Text => self.dom.text(c).is_none_or(|t| t.is_empty()),
                _ => true,
            }),
            P::FirstChild => self.prev_element(el).is_none(),
            P::LastChild => self.next_element(el).is_none(),
            P::OnlyChild => self.prev_element(el).is_none() && self.next_element(el).is_none(),
            P::FirstOfType | P::LastOfType | P::OnlyOfType => {
                let same = |x: NodeId| {
                    self.dom.local_name(x) == self.dom.local_name(el)
                        && self.dom.namespace(x) == self.dom.namespace(el)
                };
                let none_before = || {
                    std::iter::successors(self.prev_element(el), |&x| self.prev_element(x))
                        .all(|x| !same(x))
                };
                let none_after = || {
                    std::iter::successors(self.next_element(el), |&x| self.next_element(x))
                        .all(|x| !same(x))
                };
                match p {
                    P::FirstOfType => none_before(),
                    P::LastOfType => none_after(),
                    _ => none_before() && none_after(),
                }
            }
            P::Link | P::AnyLink => {
                (self.html(el, atom!("a")) || self.html(el, atom!("area")))
                    && self.has_attr(el, atom!("href"))
            }
            P::Checked => {
                self.checkable(el) && self.has_attr(el, atom!("checked"))
                    || self.html(el, atom!("option")) && self.has_attr(el, atom!("selected"))
            }
            P::Default => {
                self.checkable(el) && self.has_attr(el, atom!("checked"))
                    || self.html(el, atom!("option")) && self.has_attr(el, atom!("selected"))
            }
            P::Disabled => self.disabled(el),
            P::Enabled => self.can_disable(el) && !self.disabled(el),
            P::Indeterminate => {
                self.html(el, atom!("progress")) && !self.has_attr(el, atom!("value"))
            }
            P::Required => self.can_require(el) && self.has_attr(el, atom!("required")),
            P::Optional => self.can_require(el) && !self.has_attr(el, atom!("required")),
            P::ReadWrite => self.read_write(el),
            P::ReadOnly => !self.read_write(el),
            P::PlaceholderShown => self.placeholder_shown(el),
            P::Invalid => self.invalid(el),
            P::Valid => self.validates(el) && !self.invalid(el),
            P::Open => {
                (self.html(el, atom!("details")) || self.html(el, atom!("dialog")))
                    && self.has_attr(el, atom!("open"))
            }
            P::Defined => {
                !self.is_html(el) || self.dom.local_name(el).is_some_and(|n| !atom_has_dash(n))
            }
            P::Active
            | P::Autofill
            | P::Focus
            | P::FocusVisible
            | P::FocusWithin
            | P::Fullscreen
            | P::Hover
            | P::InRange
            | P::Modal
            | P::OutOfRange
            | P::PopoverOpen
            | P::Target
            | P::UserInvalid
            | P::UserValid
            | P::Visited => false,
        }
    }

    fn can_disable(&self, el: NodeId) -> bool {
        [
            atom!("button"),
            atom!("input"),
            atom!("select"),
            atom!("textarea"),
            atom!("optgroup"),
            atom!("option"),
            atom!("fieldset"),
        ]
        .into_iter()
        .any(|n| self.html(el, n))
    }

    fn disabled(&self, el: NodeId) -> bool {
        if !self.can_disable(el) {
            return false;
        }
        if self.has_attr(el, atom!("disabled")) {
            return true;
        }
        if self.html(el, atom!("option")) {
            return self.parent_element(el).is_some_and(|p| {
                self.html(p, atom!("optgroup")) && self.has_attr(p, atom!("disabled"))
            });
        }
        if self.html(el, atom!("optgroup")) {
            return false;
        }
        // a disabled fieldset disables what is in it, but for what is in
        // its first legend
        let mut child = el;
        let mut x = self.parent_element(el);
        while let Some(f) = x {
            if self.html(f, atom!("fieldset")) && self.has_attr(f, atom!("disabled")) {
                let first_legend = self
                    .dom
                    .children(f)
                    .find(|&c| self.html(c, atom!("legend")));
                if first_legend != Some(child) {
                    return true;
                }
            }
            child = f;
            x = self.parent_element(f);
        }
        false
    }

    fn can_require(&self, el: NodeId) -> bool {
        if self.html(el, atom!("select")) || self.html(el, atom!("textarea")) {
            return true;
        }
        self.html(el, atom!("input"))
            && !matches!(
                &*self.input_type(el),
                "hidden" | "range" | "color" | "submit" | "image" | "reset" | "button"
            )
    }

    fn text_field(&self, el: NodeId) -> bool {
        self.html(el, atom!("textarea"))
            || self.html(el, atom!("input")) && TEXT_TYPES.contains(&&*self.input_type(el))
    }

    fn read_write(&self, el: NodeId) -> bool {
        if self.text_field(el) {
            return !self.has_attr(el, atom!("readonly")) && !self.disabled(el);
        }
        let mut x = Some(el);
        while let Some(n) = x {
            if let Some(v) = self.dom.attribute(n, atom!("contenteditable")) {
                let v = text(v).to_ascii_lowercase();
                return matches!(&*v, "" | "true" | "plaintext-only");
            }
            x = self.parent_element(n);
        }
        false
    }

    /// The value the control starts with: the `value` attribute, or a
    /// textarea's text.
    fn empty_value(&self, el: NodeId) -> bool {
        if self.html(el, atom!("textarea")) {
            return self
                .dom
                .children(el)
                .all(|c| self.dom.text(c).is_none_or(|t| t.is_empty()));
        }
        self.dom
            .attribute(el, atom!("value"))
            .is_none_or(|v| v.is_empty())
    }

    fn placeholder_shown(&self, el: NodeId) -> bool {
        self.text_field(el) && self.has_attr(el, atom!("placeholder")) && self.empty_value(el)
    }

    /// A candidate for constraint validation, or a form or fieldset,
    /// which are valid when what is in them is.
    fn validates(&self, el: NodeId) -> bool {
        self.html(el, atom!("form"))
            || self.html(el, atom!("fieldset"))
            || self.can_require(el) && !self.disabled(el)
    }

    fn invalid(&self, el: NodeId) -> bool {
        if self.html(el, atom!("form")) || self.html(el, atom!("fieldset")) {
            return self.dom.descendants(el).any(|d| {
                self.dom.kind(d) == NodeKind::Element
                    && !self.html(d, atom!("form"))
                    && !self.html(d, atom!("fieldset"))
                    && self.invalid(d)
            });
        }
        if !self.validates(el) || !self.has_attr(el, atom!("required")) {
            return false;
        }
        if self.checkable(el) {
            return !self.has_attr(el, atom!("checked"));
        }
        if self.html(el, atom!("select")) {
            return false;
        }
        self.empty_value(el)
    }

    pub(crate) fn dir(&self, el: NodeId) -> Dir {
        let mut x = Some(el);
        while let Some(n) = x {
            if self.is_html(n)
                && let Some(d) = self.dom.attribute(n, atom!("dir"))
            {
                match &*text(d).to_ascii_lowercase() {
                    "rtl" => return Dir::Rtl,
                    "ltr" | "auto" => return Dir::Ltr,
                    _ => {}
                }
            }
            x = self.parent_element(n);
        }
        Dir::Ltr
    }
}

fn atom_has_dash(a: Atom) -> bool {
    let mut v = Vec::new();
    a.write_wtf8(&mut v);
    v.contains(&b'-')
}
