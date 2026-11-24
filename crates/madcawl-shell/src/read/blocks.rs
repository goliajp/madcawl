//! Regions, links, and preformatted text.

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_html::Namespace;
use madcawl_str::{Atom, atom};

use super::text::{self, Line, with_str};
use super::{Kind, Reader};

impl Reader<'_> {
    pub(crate) fn region(&mut self, el: NodeId, depth: u16, role: &'static str) {
        self.flush();
        let at = self.items.len();
        let mut text = String::new();
        if let Some(name) = self.label(el) {
            text::quoted(&mut text, &name);
        }
        self.push(depth, Kind::Region(role), el, text);
        self.walk(el, depth + 1, el);
        self.flush();
        if self.items.len() == at + 1 {
            self.items.pop();
        }
    }

    pub(crate) fn html_region(&self, el: NodeId, n: Atom) -> Option<&'static str> {
        Some(match n {
            n if n == atom!("nav") => "navigation",
            n if n == atom!("main") => "main",
            n if n == atom!("aside") => "complementary",
            n if n == atom!("form") => "form",
            n if n == atom!("dialog") => "dialog",
            n if n == atom!("search") => "search",
            n if n == atom!("article") => "article",
            n if n == atom!("section") && self.dom.attribute(el, atom!("aria-label")).is_some() => {
                "region"
            }
            n if n == atom!("header") && !self.in_sectioning(el) => "banner",
            n if n == atom!("footer") && !self.in_sectioning(el) => "contentinfo",
            _ => return None,
        })
    }

    /// Inside sectioning content, where `header` and `footer` are not
    /// landmarks.
    fn in_sectioning(&self, el: NodeId) -> bool {
        let sectioning = [
            atom!("article"),
            atom!("aside"),
            atom!("main"),
            atom!("nav"),
            atom!("section"),
        ];
        std::iter::successors(self.dom.parent(el), |&p| self.dom.parent(p)).any(|p| {
            self.dom
                .local_name(p)
                .is_some_and(|n| sectioning.contains(&n))
        })
    }

    pub(crate) fn link(&mut self, el: NodeId, depth: u16, block: NodeId) {
        let href = self
            .dom
            .attribute(el, atom!("href"))
            .map(text::string)
            .unwrap_or_default();
        let mut line = Line::default();
        let heading = self.flat(el, &mut line);
        // what is shown comes first: it is what a person reads
        let name = Some(line.s)
            .filter(|s| !s.is_empty())
            .or_else(|| self.attr(el, atom!("aria-label")))
            .or_else(|| self.image_names(el))
            .or_else(|| self.attr(el, atom!("title")))
            .unwrap_or_default();
        let token = text::link(&name, &href, el);
        self.used.push(el);
        match heading {
            Some(level) => self.push(depth, Kind::Heading(level), el, token),
            None => self.line(depth, block).raw(&token),
        }
    }

    /// The names of the shown images below `el` that have no text: an
    /// svg's `aria-label` or `<title>`, and `role="img"`'s `aria-label`.
    fn image_names(&mut self, el: NodeId) -> Option<String> {
        let mut line = Line::default();
        let mut named: Option<NodeId> = None;
        for n in self.dom.descendants(el) {
            if self.dom.kind(n) != NodeKind::Element || !self.shown.shown(n) {
                continue;
            }
            // one name per image: an svg inside a named role="img" has none
            let dom = self.dom;
            if named.is_some_and(|a| {
                std::iter::successors(dom.parent(n), |&p| dom.parent(p)).any(|p| p == a)
            }) {
                continue;
            }
            let img = self
                .attr(n, atom!("role"))
                .is_some_and(|r| r.eq_ignore_ascii_case("img"));
            if !img && !is_svg(dom, n) {
                continue;
            }
            let name = self.attr(n, atom!("aria-label")).or_else(|| {
                let mut s = String::new();
                for c in self.dom.children(svg_title(dom, n)?) {
                    if let Some(v) = self.dom.text(c) {
                        with_str(v, |v| s.push_str(v));
                    }
                }
                Some(s)
            });
            if let Some(name) = name.filter(|s| !s.trim().is_empty()) {
                line.text(&name);
                line.space();
                self.used.push(n);
                named = Some(n);
            }
        }
        Some(line.s).filter(|s| !s.is_empty())
    }

    pub(crate) fn pre(&mut self, el: NodeId, depth: u16) {
        let mut s = String::new();
        self.raw_text(el, &mut s);
        let s = s.trim_end().to_owned();
        if !s.trim().is_empty() {
            self.push(depth, Kind::Pre, el, s);
        }
    }

    /// The shown text below `el` as it is, white space kept.
    pub(crate) fn raw_text(&mut self, el: NodeId, out: &mut String) {
        for c in self.dom.children(el) {
            match self.dom.kind(c) {
                NodeKind::Text if self.shown.shown(c) => {
                    if let Some(t) = self.dom.text(c) {
                        with_str(t, |s| out.push_str(s));
                        self.used.push(c);
                    }
                }
                NodeKind::Element if self.shown.rendered(c) => {
                    if self.dom.local_name(c) == Some(atom!("br")) {
                        out.push('\n');
                    }
                    self.raw_text(c, out);
                }
                _ => {}
            }
        }
    }
}

fn is_svg(dom: &Dom, n: NodeId) -> bool {
    dom.namespace(n) == Some(Namespace::Svg) && dom.local_name(n) == Some(atom!("svg"))
}

/// The `<title>` child of an svg element.
pub fn svg_title(dom: &Dom, n: NodeId) -> Option<NodeId> {
    is_svg(dom, n).then_some(())?;
    dom.children(n).find(|&c| {
        dom.namespace(c) == Some(Namespace::Svg) && dom.local_name(c) == Some(atom!("title"))
    })
}
