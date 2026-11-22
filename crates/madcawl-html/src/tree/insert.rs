//! Creating and inserting nodes.

use super::names::svg_tag;
use super::sink::{Adjust, Attrs};
use super::stack::MAX_TREE_DEPTH;
use super::tag::Tag;
use super::{Namespace, Open, Start, TreeBuilder, TreeSink};
use crate::error::ParseError;

type Place<H> = (H, Option<H>);

pub(crate) fn is_html_ip(ns: Namespace, s: &Start<'_>) -> bool {
    match ns {
        Namespace::MathMl => {
            s.name.tag == Tag::AnnotationXml
                && s.attr(b"encoding").is_some_and(|v| {
                    v.eq_ignore_ascii_case(b"text/html")
                        || v.eq_ignore_ascii_case(b"application/xhtml+xml")
                })
        }
        Namespace::Svg => matches!(s.name.tag, Tag::ForeignObject | Tag::Desc | Tag::Title),
        Namespace::Html => false,
    }
}

impl<S: TreeSink> TreeBuilder<S> {
    pub(crate) fn current(&self) -> &Open<S::Handle> {
        self.open
            .last()
            .expect("the stack of open elements is not empty")
    }

    /// The appropriate place for inserting a node, in `target` or else
    /// in the current node.
    pub(crate) fn place(&mut self, target: Option<Open<S::Handle>>) -> Place<S::Handle> {
        let at_current = target.is_none();
        let mut target = target.unwrap_or(*self.current());
        if self.foster
            && target.ns == Namespace::Html
            && matches!(
                target.name.tag,
                Tag::Table | Tag::Tbody | Tag::Tfoot | Tag::Thead | Tag::Tr
            )
        {
            let last = self
                .open
                .iter()
                .rposition(|e| e.html(Tag::Template) || e.html(Tag::Table));
            let Some(i) = last else {
                return (self.open[0].node, None);
            };
            let e = self.open[i];
            if e.html(Tag::Template) {
                target = e;
            } else if let Some(parent) = self.sink.parent(e.node) {
                return (parent, Some(e.node));
            } else {
                target = self.open[i - 1];
            }
        }
        if target.html(Tag::Template) {
            return (self.sink.template_contents(target.node), None);
        }
        // the html element is the first entry and does not count
        if at_current
            && self.open.len() > MAX_TREE_DEPTH + 1
            && let Some(parent) = self.sink.parent(target.node)
        {
            if !self.depth_capped {
                self.depth_capped = true;
                self.sink.parse_error(ParseError::TreeTooDeep);
            }
            return (parent, None);
        }
        (target.node, None)
    }

    pub(crate) fn insert_at(&mut self, (parent, before): Place<S::Handle>, child: S::Handle) {
        match before {
            Some(b) => self.sink.insert_before(parent, child, b),
            None => self.sink.append(parent, child),
        }
    }

    /// Create an element for a token.
    pub(crate) fn create(
        &mut self,
        s: &Start<'_>,
        ns: Namespace,
        intended_parent: S::Handle,
    ) -> S::Handle {
        let name = s.name.atom;
        let (local, adjust) = match ns {
            Namespace::Html => (name, Adjust::Html),
            Namespace::Svg => (svg_tag(name).unwrap_or(name), Adjust::Svg),
            Namespace::MathMl => (name, Adjust::MathMl),
        };
        let el = self
            .sink
            .create_element(ns, local, Attrs::new(s.attrs, adjust, &self.names));
        if ns == Namespace::Html
            && let Some(form) = self.form
            && self.context.is_none()
            && !self.in_template_contents()
            && form_associated(s)
        {
            self.sink.associate_form(el, form, intended_parent);
        }
        el
    }

    /// Insert a foreign element (or an HTML one) for a token and push it.
    pub(crate) fn insert_element(&mut self, s: &Start<'_>, ns: Namespace) -> S::Handle {
        self.flush_text();
        let at = self.place(None);
        let el = self.create(s, ns, at.0);
        self.insert_at(at, el);
        self.open.push(Open {
            node: el,
            ns,
            name: s.name,
            html_ip: is_html_ip(ns, s),
        });
        el
    }

    pub(crate) fn insert_html(&mut self, s: &Start<'_>) -> S::Handle {
        self.insert_element(s, Namespace::Html)
    }

    pub(crate) fn insert_fake(&mut self, tag: Tag) -> S::Handle {
        self.insert_html(&Start::fake(tag))
    }

    /// Insert an element and pop it at once, as for void elements.
    pub(crate) fn insert_void(&mut self, s: &Start<'_>) {
        self.insert_html(s);
        self.pop();
    }

    pub(crate) fn insert_chars(&mut self, s: &[u8]) {
        if s.is_empty() {
            return;
        }
        let at = self.place(None);
        if self.text_at.is_some_and(|p| p != at) {
            self.flush_text();
        }
        self.text_at = Some(at);
        self.text.extend_from_slice(s);
    }

    pub(crate) fn flush_text(&mut self) {
        if let Some((parent, before)) = self.text_at.take() {
            self.sink.insert_text(parent, before, &self.text);
            self.text.clear();
        }
    }

    pub(crate) fn insert_comment(&mut self, data: &[u8], at: Option<Place<S::Handle>>) {
        let at = match at {
            Some(at) => at,
            None => self.place(None),
        };
        let c = self.sink.create_comment(data);
        self.insert_at(at, c);
    }

    pub(crate) fn insert_pi(&mut self, target: &[u8], data: &[u8], at: Option<Place<S::Handle>>) {
        let at = match at {
            Some(at) => at,
            None => self.place(None),
        };
        let pi = self.sink.create_processing_instruction(target, data);
        self.insert_at(at, pi);
    }

    /// The generic raw text and RCDATA element parsing algorithms.
    pub(crate) fn parse_text_element(&mut self, s: &Start<'_>, state: crate::State) {
        self.insert_html(s);
        self.next_state = Some(state);
        self.original_mode = self.mode;
        self.mode = super::Mode::Text;
    }

    pub(crate) fn in_template_contents(&self) -> bool {
        self.open.html_open(Tag::Template) > 0
            || self.context.is_some_and(|c| c.html(Tag::Template))
    }
}

/// A form-associated element that the form element pointer applies to:
/// not a listed one with its own `form` attribute. `label` is labelable
/// but not form-associated: its `form` IDL attribute reads the form of
/// the control it labels. Form-associated custom elements are not known
/// at parse time without a custom element registry.
fn form_associated(s: &Start<'_>) -> bool {
    match s.name.tag {
        Tag::Button
        | Tag::Fieldset
        | Tag::Input
        | Tag::Object
        | Tag::Output
        | Tag::Select
        | Tag::Textarea => s.attr(b"form").is_none(),
        Tag::Img => true,
        _ => false,
    }
}
