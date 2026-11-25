//! States and values: from the ARIA attributes and the HTML elements'
//! own (`disabled`, `checked`, `required`, an input's value...).

use madcawl_dom::{NodeId, NodeKind};
use madcawl_str::{Atom, atom};

use crate::cx::Cx;
use crate::html_table::is_html;
use crate::record::*;
use crate::role::Role;
use crate::text::{attr_string, collapsed, text_content};

/// What a password field shows for any value.
pub const MASK: &str = "••••••";

impl Cx<'_> {
    /// The state flags of element `n` with role `role`, and its value.
    pub fn states(&self, n: NodeId, role: Role) -> (u16, Option<String>) {
        if self.dom.kind(n) != NodeKind::Element {
            return (0, None);
        }
        let dom = self.dom;
        let has = |a: Atom| dom.attribute(n, a).is_some();
        let aria = |a: Atom| attr_string(dom, n, a).map(|v| v.trim().to_ascii_lowercase());
        let mut f = 0u16;
        let mut set = |cond: bool, flag: u16| {
            if cond {
                f |= flag;
            }
        };
        set(
            self.disabled(n) || aria(atom!("aria-disabled")).as_deref() == Some("true"),
            DISABLED,
        );
        let input_ty =
            is_html(dom, n, atom!("input")).then(|| aria(atom!("type")).unwrap_or_default());
        let native_check =
            matches!(input_ty.as_deref(), Some("checkbox" | "radio")) && has(atom!("checked"));
        match aria(atom!("aria-checked")).as_deref() {
            Some("true") => set(true, CHECKED),
            Some("mixed") => set(true, MIXED),
            _ => set(native_check, CHECKED),
        }
        match aria(atom!("aria-pressed")).as_deref() {
            Some("true") => set(true, PRESSED),
            Some("mixed") => set(true, MIXED),
            _ => {}
        }
        match aria(atom!("aria-expanded")).as_deref() {
            Some("true") => set(true, EXPANDED),
            Some("false") => set(true, COLLAPSED),
            _ => {}
        }
        if is_html(dom, n, atom!("details")) {
            set(has(atom!("open")), EXPANDED);
            set(!has(atom!("open")), COLLAPSED);
        }
        set(
            has(atom!("required")) || aria(atom!("aria-required")).as_deref() == Some("true"),
            REQUIRED,
        );
        set(
            has(atom!("readonly")) || aria(atom!("aria-readonly")).as_deref() == Some("true"),
            READONLY,
        );
        let selected = role == Role::Option && has(atom!("selected"))
            || aria(atom!("aria-selected")).as_deref() == Some("true");
        set(selected, SELECTED);
        set(
            aria(atom!("aria-current")).is_some_and(|v| v != "false" && !v.is_empty()),
            CURRENT,
        );
        let password = input_ty.as_deref() == Some("password");
        set(password, PASSWORD);
        (f, self.value_of(n, role, password))
    }

    fn disabled(&self, n: NodeId) -> bool {
        let dom = self.dom;
        let names = [
            atom!("button"),
            atom!("input"),
            atom!("select"),
            atom!("textarea"),
            atom!("optgroup"),
            atom!("option"),
            atom!("fieldset"),
        ];
        let can = names.iter().any(|&a| is_html(dom, n, a));
        if !can {
            return false;
        }
        if dom.attribute(n, atom!("disabled")).is_some() {
            return true;
        }
        // a disabled fieldset disables what it holds, but its first legend
        let mut child = n;
        let mut up = dom.parent(n);
        while let Some(p) = up {
            if is_html(dom, p, atom!("fieldset")) && dom.attribute(p, atom!("disabled")).is_some() {
                let legend = dom.children(p).find(|&c| is_html(dom, c, atom!("legend")));
                return legend != Some(child);
            }
            child = p;
            up = dom.parent(p);
        }
        false
    }

    fn value_of(&self, n: NodeId, role: Role, password: bool) -> Option<String> {
        let dom = self.dom;
        if password {
            return Some(MASK.to_owned());
        }
        match role {
            Role::Link => attr_string(dom, n, atom!("href")),
            Role::TextBox | Role::SearchBox | Role::Combobox | Role::SpinButton => {
                if is_html(dom, n, atom!("textarea")) {
                    return Some(text_content(dom, n)).filter(|v| !v.is_empty());
                }
                if is_html(dom, n, atom!("select")) {
                    return self.selected_text(n);
                }
                attr_string(dom, n, atom!("value")).filter(|v| !v.is_empty())
            }
            Role::ListBox if is_html(dom, n, atom!("select")) => self.selected_text(n),
            Role::Slider | Role::ProgressBar | Role::Meter | Role::ScrollBar => {
                attr_string(dom, n, atom!("aria-valuetext"))
                    .or_else(|| attr_string(dom, n, atom!("aria-valuenow")))
                    .or_else(|| attr_string(dom, n, atom!("value")))
            }
            _ => None,
        }
    }

    fn selected_text(&self, n: NodeId) -> Option<String> {
        let dom = self.dom;
        let mut first = None;
        for o in dom.descendants(n) {
            if !is_html(dom, o, atom!("option")) {
                continue;
            }
            if dom.attribute(o, atom!("selected")).is_some() {
                return Some(collapsed(&text_content(dom, o)));
            }
            first.get_or_insert(o);
        }
        first.map(|o| collapsed(&text_content(dom, o)))
    }
}
