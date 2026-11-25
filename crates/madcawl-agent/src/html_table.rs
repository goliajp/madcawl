//! The HTML elements whose role does not depend on where they are.

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_html::Namespace;
use madcawl_str::{Atom, atom};

use crate::cx::Cx;
use crate::role::Role;
use crate::text::attr_string;

/// The roles HTML-AAM gives an element whatever is around it.
pub(crate) fn simple_role(name: Atom) -> Option<Role> {
    const TABLE: &[(Atom, Role)] = &[
        (atom!("address"), Role::Group),
        (atom!("article"), Role::Article),
        (atom!("blockquote"), Role::Blockquote),
        (atom!("button"), Role::Button),
        (atom!("caption"), Role::Caption),
        (atom!("code"), Role::Code),
        (atom!("datalist"), Role::ListBox),
        (atom!("dd"), Role::Definition),
        (atom!("del"), Role::Deletion),
        (atom!("s"), Role::Deletion),
        (atom!("details"), Role::Group),
        (atom!("dfn"), Role::Term),
        (atom!("dt"), Role::Term),
        (atom!("dialog"), Role::Dialog),
        (atom!("em"), Role::Emphasis),
        (atom!("fieldset"), Role::Group),
        (atom!("figure"), Role::Figure),
        (atom!("h1"), Role::Heading),
        (atom!("h2"), Role::Heading),
        (atom!("h3"), Role::Heading),
        (atom!("h4"), Role::Heading),
        (atom!("h5"), Role::Heading),
        (atom!("h6"), Role::Heading),
        (atom!("hgroup"), Role::Group),
        (atom!("hr"), Role::Separator),
        (atom!("html"), Role::Document),
        (atom!("ins"), Role::Insertion),
        (atom!("main"), Role::Main),
        (atom!("mark"), Role::Mark),
        (atom!("math"), Role::Math),
        (atom!("menu"), Role::List),
        (atom!("ol"), Role::List),
        (atom!("ul"), Role::List),
        (atom!("dir"), Role::List),
        (atom!("meter"), Role::Meter),
        (atom!("nav"), Role::Navigation),
        (atom!("optgroup"), Role::Group),
        (atom!("option"), Role::Option),
        (atom!("output"), Role::Status),
        (atom!("p"), Role::Paragraph),
        (atom!("progress"), Role::ProgressBar),
        (atom!("search"), Role::Search),
        (atom!("strong"), Role::Strong),
        (atom!("summary"), Role::Button),
        (atom!("sub"), Role::Subscript),
        (atom!("sup"), Role::Superscript),
        (atom!("table"), Role::Table),
        (atom!("tbody"), Role::RowGroup),
        (atom!("thead"), Role::RowGroup),
        (atom!("tfoot"), Role::RowGroup),
        (atom!("textarea"), Role::TextBox),
        (atom!("time"), Role::Time),
        (atom!("tr"), Role::Row),
    ];
    TABLE.iter().find(|(a, _)| *a == name).map(|&(_, r)| r)
}

impl Cx<'_> {
    /// A heading's level: `aria-level`, else the `h1`-`h6` number, else 2.
    pub(crate) fn level(&self, n: NodeId) -> u8 {
        if let Some(l) = attr_string(self.dom, n, atom!("aria-level"))
            .and_then(|v| v.trim().parse::<u8>().ok())
            .filter(|&l| l > 0)
        {
            return l;
        }
        let names = [
            atom!("h1"),
            atom!("h2"),
            atom!("h3"),
            atom!("h4"),
            atom!("h5"),
            atom!("h6"),
        ];
        self.dom
            .local_name(n)
            .filter(|_| self.dom.in_html_namespace(n))
            .and_then(|a| names.iter().position(|&h| h == a))
            .map_or(2, |i| i as u8 + 1)
    }
}

/// Whether `dom`'s element `n` is an HTML element named `name`.
pub(crate) fn is_html(dom: &Dom, n: NodeId, name: Atom) -> bool {
    dom.kind(n) == NodeKind::Element
        && dom.namespace(n) == Some(Namespace::Html)
        && dom.local_name(n) == Some(name)
}
