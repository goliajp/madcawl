//! The ARIA `role` attribute, where it makes an element a widget, a
//! heading, a list item or a region.

use madcawl_dom::NodeId;
use madcawl_str::atom;

use super::text::{Line, quoted};
use super::{Kind, Reader};

/// The roles read as one control line, with the states they may have.
fn widget(role: &str) -> Option<&'static str> {
    Some(match role {
        "button" => "button",
        "link" => "link",
        "checkbox" => "checkbox",
        "radio" => "radio",
        "switch" => "switch",
        "tab" => "tab",
        "menuitem" => "menuitem",
        "menuitemcheckbox" => "menuitemcheckbox",
        "menuitemradio" => "menuitemradio",
        "option" => "option",
        "textbox" => "textbox",
        "searchbox" => "searchbox",
        "combobox" => "combobox",
        "slider" => "slider",
        "spinbutton" => "spinbutton",
        _ => return None,
    })
}

fn region(role: &str) -> Option<&'static str> {
    Some(match role {
        "navigation" => "navigation",
        "main" => "main",
        "banner" => "banner",
        "contentinfo" => "contentinfo",
        "complementary" => "complementary",
        "search" => "search",
        "form" => "form",
        "dialog" => "dialog",
        "alertdialog" => "alertdialog",
        "region" => "region",
        "article" => "article",
        _ => return None,
    })
}

const STATES: [(&str, madcawl_str::Atom); 6] = [
    ("checked", atom!("aria-checked")),
    ("selected", atom!("aria-selected")),
    ("expanded", atom!("aria-expanded")),
    ("pressed", atom!("aria-pressed")),
    ("disabled", atom!("aria-disabled")),
    ("current", atom!("aria-current")),
];

impl Reader<'_> {
    /// Reads `el` by its role; false when the role changes nothing
    /// here.
    pub(crate) fn role(&mut self, el: NodeId, role: &str, depth: u16) -> bool {
        let role = role
            .split_ascii_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if let Some(w) = widget(&role) {
            self.widget(el, w, depth);
            return true;
        }
        if let Some(r) = region(&role) {
            if r != "region" || self.label(el).is_some() {
                self.region(el, depth, r);
                return true;
            }
            return false;
        }
        match role.as_str() {
            "heading" => {
                let level = self
                    .attr(el, atom!("aria-level"))
                    .and_then(|l| l.parse::<u8>().ok())
                    .filter(|l| (1..=6).contains(l))
                    .unwrap_or(2);
                self.within(el, depth, Kind::Heading(level));
                true
            }
            "listitem" => {
                self.within(el, depth, Kind::ListItem);
                true
            }
            _ => false,
        }
    }

    /// One line: the role, the shown text or else the label, the states.
    fn widget(&mut self, el: NodeId, w: &'static str, depth: u16) {
        let mut line = Line::default();
        self.flat(el, &mut line);
        let name = match line.s.is_empty() {
            true => self.label(el).unwrap_or_default(),
            false => line.s,
        };
        let mut text = String::new();
        quoted(&mut text, &name);
        for (state, a) in STATES {
            if let Some(v) = self.attr(el, a).filter(|v| v != "false") {
                text.push(' ');
                text.push_str(state);
                if v != "true" {
                    text.push('=');
                    text.push_str(&v);
                }
            }
        }
        if let Some(href) = self.attr(el, atom!("href")) {
            text.push_str(" href=");
            quoted(&mut text, &href);
        }
        self.push(depth, Kind::Control(w), el, text);
    }
}
