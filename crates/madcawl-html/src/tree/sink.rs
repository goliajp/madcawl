//! What the tree builder builds through: a document implements
//! `TreeSink`, and the builder calls it with nodes it has decided on.

use madcawl_str::{Atom, HeldAtoms};

use crate::error::ParseError;
use crate::token::Attribute;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Namespace {
    Html,
    Svg,
    MathMl,
}

impl Namespace {
    pub fn url(self) -> &'static str {
        match self {
            Namespace::Html => "http://www.w3.org/1999/xhtml",
            Namespace::Svg => "http://www.w3.org/2000/svg",
            Namespace::MathMl => "http://www.w3.org/1998/Math/MathML",
        }
    }
}

/// The namespace of an attribute. Only foreign elements get the
/// namespaced ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrNamespace {
    None,
    XLink,
    Xml,
    Xmlns,
}

impl AttrNamespace {
    pub fn url(self) -> Option<&'static str> {
        match self {
            AttrNamespace::None => None,
            AttrNamespace::XLink => Some("http://www.w3.org/1999/xlink"),
            AttrNamespace::Xml => Some("http://www.w3.org/XML/1998/namespace"),
            AttrNamespace::Xmlns => Some("http://www.w3.org/2000/xmlns/"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuirksMode {
    NoQuirks,
    LimitedQuirks,
    Quirks,
}

/// One attribute as it goes on the element: name case and namespace
/// already adjusted for foreign content. The atoms stay valid for as
/// long as the parser runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttrRef<'a> {
    pub ns: AttrNamespace,
    pub prefix: Option<Atom>,
    pub local: Atom,
    pub value: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Adjust {
    Html,
    Svg,
    MathMl,
}

/// The attributes of a start tag, adjusted as they are read.
#[derive(Clone, Copy, Debug)]
pub struct Attrs<'a> {
    list: &'a [Attribute],
    adjust: Adjust,
    /// Where the names that are not static atoms are held.
    names: Option<&'a HeldAtoms>,
}

impl<'a> Attrs<'a> {
    pub(crate) fn new(list: &'a [Attribute], adjust: Adjust, names: &'a HeldAtoms) -> Self {
        Self {
            list,
            adjust,
            names: Some(names),
        }
    }

    pub fn empty() -> Attrs<'static> {
        Attrs {
            list: &[],
            adjust: Adjust::Html,
            names: None,
        }
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = AttrRef<'a>> + 'a {
        let (adjust, names) = (self.adjust, self.names);
        self.list.iter().map(move |a| {
            let name = a.atom.unwrap_or_else(|| {
                names
                    .expect("only an empty list has no holder")
                    .hold(&a.name)
            });
            super::names::adjust_attr(a, name, adjust)
        })
    }
}

/// A document under construction. `Handle` names a node; the builder
/// keeps handles of open elements, so they must stay valid for as long
/// as the parse runs.
///
/// Inserting a node that already has a parent moves it, as in the DOM.
pub trait TreeSink {
    type Handle: Copy + Eq;

    fn document(&self) -> Self::Handle;

    /// `local`, and the attribute names in `attrs`, stay valid for as
    /// long as the parser runs; a sink that keeps one longer holds it
    /// itself (`HeldAtoms::keep`).
    fn create_element(&mut self, ns: Namespace, local: Atom, attrs: Attrs<'_>) -> Self::Handle;
    fn create_comment(&mut self, data: &[u8]) -> Self::Handle;
    fn create_processing_instruction(&mut self, target: &[u8], data: &[u8]) -> Self::Handle;
    /// Create a DOCTYPE and append it to the document. Missing parts
    /// come as empty strings.
    fn append_doctype(&mut self, name: &[u8], public_id: &[u8], system_id: &[u8]);

    fn append(&mut self, parent: Self::Handle, child: Self::Handle);
    fn insert_before(&mut self, parent: Self::Handle, child: Self::Handle, before: Self::Handle);
    /// Add text before `before`, or at the end of `parent` when it is
    /// `None`, merging it into the text node right in front of that
    /// place if there is one.
    fn insert_text(&mut self, parent: Self::Handle, before: Option<Self::Handle>, text: &[u8]);
    fn remove(&mut self, node: Self::Handle);
    /// Move every child of `from` to the end of `to`, in order.
    fn reparent_children(&mut self, from: Self::Handle, to: Self::Handle);
    fn parent(&self, node: Self::Handle) -> Option<Self::Handle>;

    /// Add each attribute the element does not have yet, as when a
    /// stray `<html>` or `<body>` tag repeats one.
    fn add_attrs_if_missing(&mut self, element: Self::Handle, attrs: Attrs<'_>);
    /// The document fragment that holds a template element's contents.
    fn template_contents(&mut self, template: Self::Handle) -> Self::Handle;
    fn set_quirks_mode(&mut self, mode: QuirksMode);

    /// A form-associated element was created while `form` was the open
    /// form. The sink associates them if `intended_parent` is in the
    /// same tree as `form`.
    fn associate_form(
        &mut self,
        _element: Self::Handle,
        _form: Self::Handle,
        _intended_parent: Self::Handle,
    ) {
    }
    /// The parser has put every child into the element it will put
    /// there; it has left the stack of open elements.
    fn children_finished(&mut self, _element: Self::Handle) {}
    fn parse_error(&mut self, _error: ParseError) {}
    /// The document holds all the nodes or bytes it may take. The
    /// parser checks before each token and, once it is full, reports
    /// `DocumentTooLarge` and stops. A sink that says so still has room
    /// for the nodes one token creates.
    fn is_full(&self) -> bool {
        false
    }
}
