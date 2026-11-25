//! The accessible name, by the accname algorithm: `aria-labelledby`,
//! an embedded control's value, `aria-label`, the host language's label
//! (HTML-AAM: `label`, `alt`, `legend`, `caption`...), the content,
//! then `title` and `placeholder`.

use madcawl_css::style::{Content, ContentItem};
use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_str::{Atom, atom};

use crate::cx::Cx;
use crate::html_table::is_html;
use crate::role::Role;
use crate::text::{attr_filled, attr_string, collapsed, id_list, is_blank, string};

/// Where a name came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    None,
    /// What the element contains
    Content,
    /// The host language: a label, `alt`, a legend, a caption
    Native,
    /// An attribute an author wrote for readers only: `aria-label`,
    /// `aria-labelledby`, `title`, `placeholder`
    Attribute,
}

/// How deep names recurse into content and references.
const MAX_DEPTH: u32 = 64;

#[derive(Clone, Copy)]
pub(crate) struct State {
    pub recursing: bool,
    pub in_labelledby: bool,
    /// The node a labelledby or label traversal started at was hidden,
    /// so hidden nodes below count
    pub hidden_ok: bool,
    pub depth: u32,
}

pub(crate) struct Walk<'c, 'a> {
    pub cx: &'c Cx<'a>,
    pub root: NodeId,
    /// Labelledby targets being followed, against cycles
    pub stack: Vec<NodeId>,
}

impl Cx<'_> {
    /// The name of `n`, whose role is `role`, and where it came from.
    pub fn name_of(&self, n: NodeId, role: Role) -> (String, Source) {
        if self.dom.kind(n) == NodeKind::Text {
            let t = self.dom.text(n).map(string).unwrap_or_default();
            return (collapsed(&t), Source::Content);
        }
        if self.dom.kind(n) != NodeKind::Element {
            return (String::new(), Source::None);
        }
        let mut w = Walk {
            cx: self,
            root: n,
            stack: Vec::new(),
        };
        let st = State {
            recursing: false,
            in_labelledby: false,
            hidden_ok: false,
            depth: 0,
        };
        let (s, src) = w.element(n, role, st);
        let s = collapsed(&s);
        let src = if s.is_empty() { Source::None } else { src };
        (s, src)
    }
}

impl Walk<'_, '_> {
    pub(crate) fn dom(&self) -> &Dom {
        self.cx.dom
    }

    pub(crate) fn node(&mut self, n: NodeId, st: State) -> String {
        if st.depth > MAX_DEPTH {
            return String::new();
        }
        match self.dom().kind(n) {
            NodeKind::Text => {
                if !st.hidden_ok && self.cx.hidden(n) {
                    return String::new();
                }
                self.dom().text(n).map(string).unwrap_or_default()
            }
            NodeKind::Element => {
                if n == self.root && st.recursing && !st.in_labelledby {
                    // the control a label holds is not part of its own name
                    return String::new();
                }
                let role = self.cx.role_of(n).0;
                self.element(n, role, st).0
            }
            _ => String::new(),
        }
    }

    fn element(&mut self, n: NodeId, role: Role, st: State) -> (String, Source) {
        let is_root = n == self.root;
        if !is_root && !st.hidden_ok && self.cx.hidden(n) {
            // a `visibility: visible` descendant of a hidden element is seen
            if self.cx.only_invisible(n) && st.recursing {
                return (self.content(n, st), Source::Content);
            }
            return (String::new(), Source::None);
        }
        if !st.in_labelledby
            && let Some(s) = self.labelledby(n, st)
        {
            return (s, Source::Attribute);
        }
        if is_root && role == Role::Presentation {
            return (String::new(), Source::None);
        }
        if !is_root && let Some(v) = self.embedded_value(n, role) {
            return (v, Source::Content);
        }
        let prohibited = is_root && role.name_prohibited();
        if !prohibited && let Some(l) = attr_filled(self.dom(), n, atom!("aria-label")) {
            return (l, Source::Attribute);
        }
        if role != Role::Presentation
            && let Some(s) = self.native(n, st)
        {
            return (s, Source::Native);
        }
        let from_content = if is_root {
            role.name_from_content()
        } else {
            st.recursing
        };
        if from_content && !prohibited {
            let s = self.content(n, st);
            // inside a name even whitespace counts: it separates words
            if !is_root || !is_blank(&s) {
                return (s, Source::Content);
            }
        }
        if let Some(t) = attr_filled(self.dom(), n, atom!("title")) {
            return (t, Source::Attribute);
        }
        let placeholder = [atom!("placeholder"), atom!("aria-placeholder")];
        if matches!(role, Role::TextBox | Role::SearchBox | Role::Combobox)
            && let Some(p) = placeholder
                .iter()
                .find_map(|&a| attr_filled(self.dom(), n, a))
        {
            return (p, Source::Attribute);
        }
        (String::new(), Source::None)
    }

    fn labelledby(&mut self, n: NodeId, st: State) -> Option<String> {
        let v = attr_string(self.dom(), n, atom!("aria-labelledby"))?;
        let mut out = String::new();
        let mut any = false;
        for id in id_list(&v) {
            let Some(t) = self.cx.ids.first(self.dom(), id) else {
                continue;
            };
            if self.stack.contains(&t) {
                continue;
            }
            any = true;
            self.stack.push(t);
            let sub = State {
                recursing: true,
                in_labelledby: true,
                hidden_ok: self.cx.hidden(t),
                depth: st.depth + 1,
            };
            let s = self.node(t, sub);
            self.stack.pop();
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&collapsed(&s));
        }
        (any && !is_blank(&out)).then_some(out)
    }

    /// The value of a control inside another's label (accname 2C).
    fn embedded_value(&mut self, n: NodeId, role: Role) -> Option<String> {
        let dom = self.dom();
        if is_html(dom, n, atom!("input"))
            && attr_string(dom, n, atom!("type"))
                .is_some_and(|t| t.trim().eq_ignore_ascii_case("password"))
        {
            // a password names nothing by its value, whatever its role
            return Some(crate::states::MASK.to_owned());
        }
        match role {
            Role::TextBox | Role::SearchBox => {
                if is_html(dom, n, atom!("textarea")) {
                    return Some(crate::text::text_content(dom, n));
                }
                attr_string(dom, n, atom!("value")).or(Some(String::new()))
            }
            Role::Combobox | Role::ListBox => {
                if is_html(dom, n, atom!("input")) {
                    return attr_string(dom, n, atom!("value")).or(Some(String::new()));
                }
                Some(self.selected_options(n))
            }
            Role::Slider | Role::SpinButton | Role::ProgressBar | Role::ScrollBar | Role::Meter => {
                attr_filled(dom, n, atom!("aria-valuetext"))
                    .or_else(|| attr_filled(dom, n, atom!("aria-valuenow")))
                    .or_else(|| attr_filled(dom, n, atom!("value")))
                    .or(Some(String::new()))
            }
            _ => None,
        }
    }

    fn selected_options(&mut self, n: NodeId) -> String {
        let dom = self.dom();
        let opts: Vec<NodeId> = dom
            .descendants(n)
            .filter(|&o| {
                let (r, _) = self.cx.role_of(o);
                r == Role::Option
            })
            .collect();
        let chosen: Vec<NodeId> = opts
            .iter()
            .copied()
            .filter(|&o| {
                dom.attribute(o, atom!("selected")).is_some()
                    || attr_string(dom, o, atom!("aria-selected")).as_deref() == Some("true")
            })
            .collect();
        if opts.is_empty() && !is_html(dom, n, atom!("select")) {
            return collapsed(&crate::text::text_content(dom, n));
        }
        let pick = if chosen.is_empty() && is_html(dom, n, atom!("select")) {
            opts.first().copied().into_iter().collect()
        } else {
            chosen
        };
        let parts: Vec<String> = pick
            .iter()
            .map(|&o| collapsed(&crate::text::text_content(dom, o)))
            .collect();
        parts.join(" ")
    }

    /// The children's names in order, and `::before` and `::after`.
    pub(crate) fn content(&mut self, n: NodeId, st: State) -> String {
        let sub = State {
            recursing: true,
            depth: st.depth + 1,
            ..st
        };
        let mut out = self.pseudo(n, false);
        let kids: Vec<NodeId> = self.dom().children(n).collect();
        for c in kids {
            let s = self.node(c, sub);
            let block = self.dom().kind(c) == NodeKind::Element && self.cx.block_level(c);
            if block {
                out.push(' ');
            }
            out.push_str(&s);
            if block {
                out.push(' ');
            }
        }
        out.push_str(&self.pseudo(n, true));
        out
    }

    fn pseudo(&self, n: NodeId, after: bool) -> String {
        let Some(s) = self.cx.styles.and_then(|s| s.pseudo(n, after)) else {
            return String::new();
        };
        let Content::Items(items) = &s.rare.content else {
            return String::new();
        };
        let mut out = String::new();
        for it in items.iter() {
            match it {
                ContentItem::Text(t) => out.push_str(t),
                ContentItem::Attr(a) => {
                    if let Some(a) = Atom::lookup(a.as_bytes())
                        && let Some(v) = attr_string(self.dom(), n, a)
                    {
                        out.push_str(&v);
                    }
                }
                _ => {}
            }
        }
        out
    }
}
