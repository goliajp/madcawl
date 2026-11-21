//! The parser's own view of tokens, open elements and modes.

use super::Namespace;
use super::tag::{Name, Tag};
use crate::token::{Attribute, Doctype};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    InHeadNoscript,
    AfterHead,
    InBody,
    Text,
    InTable,
    InTableText,
    InCaption,
    InColumnGroup,
    InTableBody,
    InRow,
    InCell,
    InTemplate,
    AfterBody,
    InFrameset,
    AfterFrameset,
    AfterAfterBody,
    AfterAfterFrameset,
}

/// An entry on the stack of open elements, with what scope checks need
/// so that they never ask the sink.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Open<H> {
    pub(crate) node: H,
    pub(crate) ns: Namespace,
    pub(crate) name: Name,
    /// An HTML integration point.
    pub(crate) html_ip: bool,
}

impl<H> Open<H> {
    pub(crate) fn html(&self, tag: Tag) -> bool {
        self.ns == Namespace::Html && self.name.tag == tag
    }

    pub(crate) fn is(&self, ns: Namespace, tag: Tag) -> bool {
        self.ns == ns && self.name.tag == tag
    }

    pub(crate) fn mathml_text_ip(&self) -> bool {
        self.ns == Namespace::MathMl
            && matches!(
                self.name.tag,
                Tag::Mi | Tag::Mo | Tag::Mn | Tag::Ms | Tag::Mtext
            )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Space,
    Nul,
    Other,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Start<'a> {
    pub(crate) name: Name,
    pub(crate) raw: &'a [u8],
    pub(crate) attrs: &'a [Attribute],
    pub(crate) self_closing: bool,
}

impl Start<'static> {
    /// A start tag the parser makes up, with no attributes.
    pub(crate) fn fake(tag: Tag) -> Self {
        Start {
            name: Name::known(tag),
            raw: tag.as_bytes(),
            attrs: &[],
            self_closing: false,
        }
    }
}

impl Start<'_> {
    pub(crate) fn attr(&self, name: &[u8]) -> Option<&[u8]> {
        self.attrs
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_slice())
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Tok<'a> {
    Text(Class, &'a [u8]),
    Start(Start<'a>),
    End(Name),
    Comment(&'a [u8]),
    Pi(&'a [u8], &'a [u8]),
    Doctype(&'a Doctype),
    Eof,
}

/// What a mode handler did with a token.
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Done,
    /// Process the token again, by the tree construction dispatcher.
    Again,
    /// Process the token again by the rules for HTML content.
    AgainHtml,
}

pub(crate) enum Active<H> {
    Marker,
    Element {
        node: H,
        tag: Tag,
        /// The attributes of the token it was made for.
        attrs: Vec<Attribute>,
    },
}
