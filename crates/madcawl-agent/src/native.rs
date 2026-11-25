//! The host language's part of a name (accname 2E, as HTML-AAM gives
//! it per element): a control's labels, a button input's value, `alt`,
//! a fieldset's legend, a figure's caption, a table's caption, an
//! optgroup's `label`, an SVG element's `<title>`.

use madcawl_dom::{NodeId, NodeKind};
use madcawl_html::Namespace;
use madcawl_str::{Atom, atom};

use crate::html_table::is_html;
use crate::name::{State, Walk};
use crate::text::{attr_filled, attr_string, collapsed, is_blank, string};

impl Walk<'_, '_> {
    pub(crate) fn native(&mut self, n: NodeId, st: State) -> Option<String> {
        let dom = self.dom();
        if dom.namespace(n) == Some(Namespace::Svg) {
            return self.child_text(n, atom!("title"), Namespace::Svg);
        }
        let name = dom.local_name(n).filter(|_| dom.in_html_namespace(n))?;
        if name == atom!("input") {
            return self.input(n, st);
        }
        let labelable = [
            atom!("select"),
            atom!("textarea"),
            atom!("button"),
            atom!("meter"),
            atom!("output"),
            atom!("progress"),
        ];
        if labelable.contains(&name) {
            return self.labels(n, st);
        }
        match name {
            _ if name == atom!("img") || name == atom!("area") => attr_filled(dom, n, atom!("alt")),
            _ if name == atom!("fieldset") => self.first_child_content(n, atom!("legend"), st),
            _ if name == atom!("figure") => self.first_child_content(n, atom!("figcaption"), st),
            _ if name == atom!("table") => self.first_child_content(n, atom!("caption"), st),
            _ if name == atom!("optgroup") => attr_filled(dom, n, atom!("label")),
            _ => None,
        }
    }

    fn input(&mut self, n: NodeId, st: State) -> Option<String> {
        let dom = self.dom();
        let ty = attr_string(dom, n, atom!("type"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        let value = attr_string(dom, n, atom!("value"));
        match ty.as_str() {
            "hidden" => None,
            "button" => self.labels(n, st).or(value.filter(|v| !is_blank(v))),
            "submit" | "reset" => {
                let default = if ty == "submit" { "Submit" } else { "Reset" };
                let v = value.unwrap_or_else(|| default.to_owned());
                self.labels(n, st).or(Some(v))
            }
            "image" => attr_filled(dom, n, atom!("alt"))
                .or_else(|| value.filter(|v| !is_blank(v)))
                .or_else(|| attr_filled(dom, n, atom!("title")))
                .or_else(|| Some("Submit".to_owned())),
            _ => self.labels(n, st),
        }
    }

    /// The labels of a labelable element: those whose `for` names its
    /// ID, then the label it is in, each once.
    fn labels(&mut self, n: NodeId, st: State) -> Option<String> {
        let dom = self.cx.dom;
        let mut list: Vec<NodeId> = Vec::new();
        if let Some(id) = self.cx.ids.value_of(n) {
            list.extend(self.cx.label_for.all(id).iter().copied());
        }
        let mut up = dom.parent(n);
        while let Some(p) = up {
            if is_html(dom, p, atom!("label")) {
                let other = attr_string(dom, p, atom!("for"))
                    .is_some_and(|f| self.cx.ids.value_of(n) != Some(f.as_str()));
                if !other && !list.contains(&p) {
                    list.push(p);
                }
                break;
            }
            up = dom.parent(p);
        }
        list.sort_by(|&a, &b| crate::index::tree_order(dom, a, b));
        // a hidden label names nothing (HTML-AAM)
        list.retain(|&l| !self.cx.hidden(l));
        let mut out = String::new();
        for l in list {
            let sub = State {
                recursing: true,
                in_labelledby: false,
                hidden_ok: false,
                depth: st.depth + 1,
            };
            let s = collapsed(&self.content(l, sub));
            if !s.is_empty() {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(&s);
            }
        }
        (!out.is_empty()).then_some(out)
    }

    fn first_child_content(&mut self, n: NodeId, child: Atom, st: State) -> Option<String> {
        let dom = self.dom();
        let c = dom
            .children(n)
            .find(|&c| is_html(dom, c, child) && !self.cx.hidden(c))?;
        let sub = State {
            recursing: true,
            depth: st.depth + 1,
            ..st
        };
        let s = self.content(c, sub);
        (!is_blank(&s)).then_some(s)
    }

    fn child_text(&self, n: NodeId, child: Atom, ns: Namespace) -> Option<String> {
        let dom = self.dom();
        let c = dom.children(n).find(|&c| {
            dom.kind(c) == NodeKind::Element
                && dom.namespace(c) == Some(ns)
                && dom.local_name(c) == Some(child)
        })?;
        let s: String = dom
            .children(c)
            .filter_map(|t| dom.text(t).filter(|_| dom.kind(t) == NodeKind::Text))
            .map(string)
            .collect();
        (!is_blank(&s)).then_some(s)
    }
}
