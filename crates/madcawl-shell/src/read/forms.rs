//! Form controls: one line each with role, accessible name, and the
//! name, value and states a filled form would need. A password's value
//! is never written.

use std::collections::HashMap;

use madcawl_dom::{NodeId, NodeKind};
use madcawl_html::Namespace;
use madcawl_str::{Atom, atom};

use super::text::{Line, quoted};
use super::{Kind, Reader};

/// The role an `input` of this type has; `None` for `hidden`.
fn input_role(ty: &str) -> Option<&'static str> {
    Some(match ty {
        "hidden" => return None,
        "search" => "searchbox",
        "checkbox" => "checkbox",
        "radio" => "radio",
        "range" => "slider",
        "number" => "spinbutton",
        "submit" | "reset" | "button" | "image" => "button",
        "file" => "file",
        "color" => "color",
        _ => "textbox",
    })
}

/// The name a browser gives a submit or reset `input` with no value.
fn default_name(name: Atom, ty: Option<&str>) -> Option<String> {
    (name == atom!("input")).then_some(())?;
    let n = match ty? {
        "submit" | "image" => "Submit",
        "reset" => "Reset",
        _ => return None,
    };
    Some(n.to_owned())
}

const FLAGS: [(&str, Atom); 5] = [
    ("checked", atom!("checked")),
    ("multiple", atom!("multiple")),
    ("required", atom!("required")),
    ("readonly", atom!("readonly")),
    ("disabled", atom!("disabled")),
];

impl Reader<'_> {
    pub(crate) fn control(&mut self, el: NodeId, name: Atom, depth: u16) {
        let ty = self.attr(el, atom!("type")).map(|t| t.to_ascii_lowercase());
        let mut value = self.attr(el, atom!("value"));
        let role = match name {
            n if n == atom!("input") => match input_role(ty.as_deref().unwrap_or("text")) {
                Some(r) => r,
                None => return,
            },
            n if n == atom!("button") => "button",
            n if n == atom!("textarea") => {
                let mut s = String::new();
                self.raw_text(el, &mut s);
                value = Some(s);
                "textbox"
            }
            _ => self.select_role(el),
        };
        let shown_name = match role {
            "button" if name == atom!("button") => {
                let mut line = Line::default();
                self.flat(el, &mut line);
                Some(line.s).filter(|s| !s.is_empty())
            }
            "button" => value.take(),
            _ => None,
        };
        let label = shown_name
            .or_else(|| self.label(el))
            .or_else(|| self.label_element(el))
            .or_else(|| self.attr(el, atom!("alt")))
            .or_else(|| self.attr(el, atom!("title")))
            .or_else(|| default_name(name, ty.as_deref()));
        if ty.as_deref() == Some("password") || (role == "button" && name != atom!("button")) {
            value = None;
        }
        if name == atom!("select") {
            value = self.selected(el);
        }
        let mut text = String::new();
        quoted(&mut text, label.as_deref().unwrap_or(""));
        self.fields(el, role, ty.as_deref(), value, &mut text);
        self.push(depth, Kind::Control(role), el, text);
        if name == atom!("select") {
            self.choices(el, depth + 1);
        }
    }

    /// ` name="…" type="…" value="…" placeholder="…"` and the flags.
    fn fields(
        &self,
        el: NodeId,
        role: &str,
        ty: Option<&str>,
        value: Option<String>,
        text: &mut String,
    ) {
        let field = |text: &mut String, k: &str, v: &str| {
            text.push(' ');
            text.push_str(k);
            text.push('=');
            quoted(text, v);
        };
        if let Some(n) = self.attr(el, atom!("name")) {
            field(text, "name", &n);
        }
        // the type, where the role does not already say it
        let says = match role {
            "textbox" => ty != Some("text"),
            "button" => matches!(ty, Some("reset" | "button" | "image")),
            _ => false,
        };
        if let Some(t) = ty.filter(|_| says) {
            field(text, "type", t);
        }
        if let Some(v) = value {
            field(text, "value", &v);
        }
        if let Some(p) = self.attr(el, atom!("placeholder")) {
            field(text, "placeholder", &p);
        }
        for (flag, a) in FLAGS {
            if self.dom.attribute(el, a).is_some() {
                text.push(' ');
                text.push_str(flag);
            }
        }
    }

    fn select_role(&self, el: NodeId) -> &'static str {
        let size = self
            .attr(el, atom!("size"))
            .and_then(|s| s.parse::<u32>().ok());
        if self.dom.attribute(el, atom!("multiple")).is_some() || size.is_some_and(|s| s > 1) {
            "listbox"
        } else {
            "combobox"
        }
    }

    /// The options of a select, in order, through its optgroups.
    fn options(&self, el: NodeId) -> Vec<NodeId> {
        self.dom
            .descendants(el)
            .filter(|&n| {
                self.dom.local_name(n) == Some(atom!("option"))
                    && self.dom.namespace(n) == Some(Namespace::Html)
            })
            .collect()
    }

    /// The text of the option a select shows: the first marked
    /// selected, else the first.
    fn selected(&mut self, el: NodeId) -> Option<String> {
        let opts = self.options(el);
        let pick = opts
            .iter()
            .copied()
            .find(|&o| self.dom.attribute(o, atom!("selected")).is_some())
            .or(opts.first().copied())
            .filter(|&o| self.shown.shown(o))?;
        let mut line = Line::default();
        self.flat(pick, &mut line);
        Some(line.s)
    }

    fn choices(&mut self, el: NodeId, depth: u16) {
        for o in self.options(el) {
            if !self.shown.shown(o) {
                continue;
            }
            let mut line = Line::default();
            self.flat(o, &mut line);
            let mut text = String::new();
            quoted(&mut text, &line.s);
            if let Some(v) = self.attr(o, atom!("value")) {
                text.push_str(" value=");
                quoted(&mut text, &v);
            }
            for (flag, a) in [
                ("selected", atom!("selected")),
                ("disabled", atom!("disabled")),
            ] {
                if self.dom.attribute(o, a).is_some() {
                    text.push(' ');
                    text.push_str(flag);
                }
            }
            self.push(depth, Kind::Choice, o, text);
        }
    }

    /// The name `aria-labelledby` or `aria-label` gives, from what of
    /// it is shown.
    pub(crate) fn label(&mut self, el: NodeId) -> Option<String> {
        if let Some(ids) = self.attr(el, atom!("aria-labelledby")) {
            let mut line = Line::default();
            for id in ids.split_ascii_whitespace() {
                if let Some(n) = self.by_id(id).filter(|&n| self.shown.rendered(n)) {
                    // many controls may name one large node: flatten it once
                    if !self.flattened.contains_key(&n) {
                        let mut l = Line::default();
                        self.flat(n, &mut l);
                        self.flattened.insert(n, l.s);
                    }
                    line.text(&self.flattened[&n]);
                    line.space();
                }
            }
            if !line.s.is_empty() {
                return Some(line.s);
            }
        }
        self.attr(el, atom!("aria-label"))
    }

    /// The shown text of the `<label>` for a control: one naming its id,
    /// else one around it.
    fn label_element(&mut self, el: NodeId) -> Option<String> {
        let by_for = self
            .dom
            .element_id(el)
            .and_then(|id| self.labels().get(&id).copied());
        let around = || {
            std::iter::successors(self.dom.parent(el), |&p| self.dom.parent(p))
                .find(|&p| self.dom.local_name(p) == Some(atom!("label")))
        };
        let l = by_for.or_else(around).filter(|&l| self.shown.rendered(l))?;
        let mut line = Line::default();
        self.flat(l, &mut line);
        Some(line.s).filter(|s| !s.is_empty())
    }

    fn by_id(&mut self, id: &str) -> Option<NodeId> {
        let a = Atom::lookup(id.as_bytes())?;
        if self.ids.is_none() {
            let mut m = HashMap::new();
            for n in self.dom.descendants(self.dom.document()) {
                if let Some(i) = self
                    .dom
                    .element_id(n)
                    .filter(|_| self.dom.kind(n) == NodeKind::Element)
                {
                    m.entry(i).or_insert(n);
                }
            }
            self.ids = Some(m);
        }
        self.ids.as_ref()?.get(&a).copied()
    }

    fn labels(&mut self) -> &HashMap<Atom, NodeId> {
        if self.labels.is_none() {
            let mut m = HashMap::new();
            for n in self.dom.descendants(self.dom.document()) {
                if self.dom.local_name(n) != Some(atom!("label")) {
                    continue;
                }
                let target = self
                    .dom
                    .attribute(n, atom!("for"))
                    .and_then(|f| super::text::with_str(f, |s| Atom::lookup(s.as_bytes())));
                if let Some(t) = target {
                    m.entry(t).or_insert(n);
                }
            }
            self.labels = Some(m);
        }
        self.labels.as_ref().expect("built above")
    }
}
