//! An element's role: the `role` attribute's first concrete role, as
//! ARIA lets it override, else the HTML-AAM mapping of the element and
//! where it is.

use madcawl_dom::{NodeId, NodeKind};
use madcawl_html::Namespace;
use madcawl_str::{Atom, atom};

use crate::cx::Cx;
use crate::html_table::is_html;
use crate::role::Role;
use crate::text::{attr_filled, attr_string, id_list};

impl Cx<'_> {
    /// The role of `n`, with the heading level for a heading.
    pub fn role_of(&self, n: NodeId) -> (Role, u8) {
        match self.dom.kind(n) {
            NodeKind::Text => return (Role::Text, 0),
            NodeKind::Document => return (Role::Document, 0),
            NodeKind::Element => {}
            _ => return (Role::None, 0),
        }
        let role = self.aria_role(n).unwrap_or_else(|| self.implicit(n));
        let level = match role {
            Role::Heading => self.level(n),
            _ => 0,
        };
        (role, level)
    }

    fn aria_role(&self, n: NodeId) -> Option<Role> {
        let v = attr_string(self.dom, n, atom!("role"))?;
        for t in id_list(&v) {
            let Some(r) = Role::from_aria(t) else {
                continue;
            };
            return match r {
                // presentation does not take on what a user can reach
                Role::Presentation | Role::Generic if self.focusable_or_global(n) => None,
                Role::Region | Role::Form if !self.author_named(n, true) => None,
                r => Some(r),
            };
        }
        None
    }

    /// What makes an element a group whatever role it is given
    /// (HTML-AAM): focusable, draggable, a popover, autofocus.
    fn focusable_or_global(&self, n: NodeId) -> bool {
        let has = |a: Atom| self.dom.attribute(n, a).is_some();
        let native = [
            atom!("input"),
            atom!("select"),
            atom!("textarea"),
            atom!("button"),
        ];
        native.iter().any(|&e| is_html(self.dom, n, e))
            || is_html(self.dom, n, atom!("a")) && has(atom!("href"))
            || has(atom!("contenteditable"))
            || has(atom!("tabindex"))
            || has(atom!("draggable"))
            || has(atom!("popover"))
            || has(atom!("autofocus"))
            || has(atom!("aria-label"))
            || has(atom!("aria-labelledby"))
    }

    /// The element has a name from its author: `aria-label`,
    /// `aria-labelledby` naming an element, or (`title_counts`) `title`.
    pub fn author_named(&self, n: NodeId, title_counts: bool) -> bool {
        if attr_filled(self.dom, n, atom!("aria-label")).is_some() {
            return true;
        }
        if let Some(v) = attr_string(self.dom, n, atom!("aria-labelledby")) {
            let named = id_list(&v).any(|id| {
                self.ids.first(self.dom, id).is_some_and(|t| {
                    !crate::text::is_blank(&crate::text::text_content(self.dom, t))
                })
            });
            if named {
                return true;
            }
        }
        title_counts && attr_filled(self.dom, n, atom!("title")).is_some()
    }

    fn html(&self, n: NodeId) -> Option<Atom> {
        (self.dom.namespace(n) == Some(Namespace::Html))
            .then(|| self.dom.local_name(n))
            .flatten()
    }

    fn implicit(&self, n: NodeId) -> Role {
        let Some(name) = self.html(n) else {
            return match self.dom.local_name(n) {
                Some(a) if a == atom!("svg") => Role::Image,
                Some(a) if a == atom!("math") => Role::Math,
                _ => Role::Generic,
            };
        };
        let group = || {
            let has = |a: Atom| self.dom.attribute(n, a).is_some();
            match has(atom!("draggable")) || has(atom!("popover")) || has(atom!("autofocus")) {
                true => Role::Group,
                false => Role::Generic,
            }
        };
        let unread = [
            atom!("head"),
            atom!("script"),
            atom!("style"),
            atom!("template"),
            atom!("meta"),
            atom!("link"),
            atom!("base"),
            atom!("title"),
        ];
        if unread.contains(&name) {
            return Role::None;
        }
        let simple = crate::html_table::simple_role(name);
        if let Some(r) = simple {
            return r;
        }
        match name {
            _ if name == atom!("a") || name == atom!("area") => self.link(n),
            _ if name == atom!("aside") => self.aside(n),
            _ if name == atom!("header") || name == atom!("footer") => self.header_footer(n, name),
            _ if name == atom!("section") => match self.author_named(n, true) {
                true => Role::Region,
                false => group(),
            },
            _ if name == atom!("img") => self.img(n),
            _ if name == atom!("input") => self.input(n),
            _ if name == atom!("select") => self.select(n),
            _ if name == atom!("li") => self.li(n),
            _ if name == atom!("th") => self.th(n),
            _ if name == atom!("td") => match self.in_table_role(n) {
                Some(Role::Grid | Role::TreeGrid) => Role::GridCell,
                _ => Role::Cell,
            },
            _ if name == atom!("form") => Role::Form,
            _ => group(),
        }
    }

    fn link(&self, n: NodeId) -> Role {
        match self.dom.attribute(n, atom!("href")) {
            Some(_) => Role::Link,
            None => Role::Generic,
        }
    }

    /// The nearest ancestor that is sectioning content or `main`, by
    /// element or role.
    fn sectioning(&self, n: NodeId) -> Option<Atom> {
        let mut up = self.dom.parent(n);
        while let Some(p) = up {
            if let Some(a) = self.html(p) {
                let names = [
                    atom!("article"),
                    atom!("aside"),
                    atom!("nav"),
                    atom!("section"),
                    atom!("main"),
                ];
                if names.contains(&a) {
                    return Some(a);
                }
            }
            up = self.dom.parent(p);
        }
        None
    }

    fn aside(&self, n: NodeId) -> Role {
        match self.sectioning(n) {
            None => Role::Complementary,
            Some(a) if a == atom!("main") => Role::Complementary,
            Some(_) if self.author_named(n, true) => Role::Complementary,
            Some(_) => Role::Generic,
        }
    }

    fn header_footer(&self, n: NodeId, name: Atom) -> Role {
        let top = self.sectioning(n).is_none();
        match (name == atom!("header"), top) {
            (true, true) => Role::Banner,
            (false, true) => Role::ContentInfo,
            (true, false) => Role::SectionHeader,
            (false, false) => Role::SectionFooter,
        }
    }

    fn img(&self, n: NodeId) -> Role {
        let alt = attr_string(self.dom, n, atom!("alt"));
        if alt.as_deref().is_some_and(crate::text::is_blank) {
            return match self.author_named(n, false) {
                true => Role::Image,
                false => Role::Presentation,
            };
        }
        let src = |a| attr_filled(self.dom, n, a).is_some();
        let named = alt.is_some_and(|a| !crate::text::is_blank(&a)) || self.author_named(n, true);
        match named || src(atom!("src")) || src(atom!("srcset")) {
            true => Role::Image,
            false => Role::Generic,
        }
    }

    fn input(&self, n: NodeId) -> Role {
        let ty = attr_string(self.dom, n, atom!("type"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        let list = self.dom.attribute(n, atom!("list")).is_some();
        match ty.as_str() {
            "button" | "image" | "reset" | "submit" => Role::Button,
            "checkbox" if self.dom.attribute(n, atom!("switch")).is_some() => Role::Switch,
            "checkbox" => Role::Checkbox,
            "radio" => Role::Radio,
            "range" => Role::Slider,
            "number" => Role::SpinButton,
            "hidden" => Role::None,
            "search" if !list => Role::SearchBox,
            "email" | "tel" | "text" | "url" | "search" | "" if list => Role::Combobox,
            "email" | "tel" | "text" | "url" | "password" | "" => Role::TextBox,
            "color" | "date" | "datetime-local" | "file" | "month" | "time" | "week" => {
                Role::Generic
            }
            _ => Role::TextBox,
        }
    }

    fn select(&self, n: NodeId) -> Role {
        let multiple = self.dom.attribute(n, atom!("multiple")).is_some();
        let size = attr_string(self.dom, n, atom!("size"))
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);
        match multiple || size > 1 {
            true => Role::ListBox,
            false => Role::Combobox,
        }
    }

    fn li(&self, n: NodeId) -> Role {
        let mut up = self.dom.parent(n);
        while let Some(p) = up {
            if self.dom.kind(p) != NodeKind::Element {
                break;
            }
            match self.role_of(p).0 {
                Role::List | Role::Menu | Role::MenuBar | Role::Directory => {
                    return Role::ListItem;
                }
                Role::Generic => up = self.dom.parent(p),
                _ => break,
            }
        }
        Role::Generic
    }

    fn th(&self, n: NodeId) -> Role {
        let scope = attr_string(self.dom, n, atom!("scope"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        match scope.as_str() {
            "row" | "rowgroup" => return Role::RowHeader,
            "col" | "colgroup" => return Role::ColumnHeader,
            _ => {}
        }
        let row = self.dom.parent(n);
        let in_head = row
            .and_then(|r| self.dom.parent(r))
            .is_some_and(|g| self.html(g) == Some(atom!("thead")));
        let has_td = row.is_some_and(|r| {
            self.dom
                .children(r)
                .any(|c| self.html(c) == Some(atom!("td")))
        });
        match !in_head && has_td {
            true => Role::RowHeader,
            false => Role::ColumnHeader,
        }
    }

    fn in_table_role(&self, n: NodeId) -> Option<Role> {
        let mut up = self.dom.parent(n);
        while let Some(p) = up {
            if self.html(p) == Some(atom!("table")) {
                return Some(self.role_of(p).0);
            }
            up = self.dom.parent(p);
        }
        None
    }
}
