//! The stack of open elements: scopes, popping, implied end tags and
//! resetting the insertion mode.

use super::tag::{Name, Tag};
use super::{Mode, Namespace, Open, TreeBuilder, TreeSink};

/// How deep the parser nests elements in the tree. Past it, an element
/// still goes on the stack of open elements but is inserted beside the
/// deepest one rather than inside it, as Blink does
/// (`kMaximumHTMLParserDOMTreeDepth`): the standard has no limit, and
/// everything that walks the tree afterwards would otherwise have to
/// survive any depth a page asks for.
pub(crate) const MAX_TREE_DEPTH: usize = 512;

const TAGS: usize = Tag::Other as usize + 1;

/// The stack of open elements, with a count of the open HTML elements
/// of each known name, so "is there a `p` (or a `template`) open at
/// all" is answered without walking a stack that a page can make as
/// deep as it likes.
pub(crate) struct OpenStack<H> {
    items: Vec<Open<H>>,
    html: [u32; TAGS],
}

impl<H> OpenStack<H> {
    pub(crate) fn new() -> Self {
        Self {
            items: Vec::new(),
            html: [0; TAGS],
        }
    }

    fn count(&mut self, e: &Open<H>, add: bool) {
        if e.ns == Namespace::Html {
            let c = &mut self.html[e.name.tag as usize];
            if add {
                *c += 1;
            } else {
                *c -= 1;
            }
        }
    }

    pub(crate) fn push(&mut self, e: Open<H>) {
        self.count(&e, true);
        self.items.push(e);
    }

    pub(crate) fn pop(&mut self) -> Option<Open<H>> {
        let e = self.items.pop()?;
        self.count(&e, false);
        Some(e)
    }

    pub(crate) fn remove(&mut self, i: usize) -> Open<H> {
        let e = self.items.remove(i);
        self.count(&e, false);
        e
    }

    pub(crate) fn insert(&mut self, i: usize, e: Open<H>) {
        self.count(&e, true);
        self.items.insert(i, e);
    }

    /// Replace the node of entry `i`, keeping its name.
    pub(crate) fn set_node(&mut self, i: usize, node: H) {
        self.items[i].node = node;
    }

    /// Open HTML elements named `tag`, anywhere on the stack.
    pub(crate) fn html_open(&self, tag: Tag) -> u32 {
        self.html[tag as usize]
    }
}

impl<H> std::ops::Deref for OpenStack<H> {
    type Target = [Open<H>];
    fn deref(&self) -> &[Open<H>] {
        &self.items
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    Default,
    ListItem,
    Button,
    Table,
}

fn default_boundary<H>(e: &Open<H>) -> bool {
    match e.ns {
        Namespace::Html => matches!(
            e.name.tag,
            Tag::Applet
                | Tag::Caption
                | Tag::Html
                | Tag::Table
                | Tag::Td
                | Tag::Th
                | Tag::Marquee
                | Tag::Object
                | Tag::Select
                | Tag::Template
        ),
        Namespace::MathMl => matches!(
            e.name.tag,
            Tag::Mi | Tag::Mo | Tag::Mn | Tag::Ms | Tag::Mtext | Tag::AnnotationXml
        ),
        Namespace::Svg => matches!(e.name.tag, Tag::ForeignObject | Tag::Desc | Tag::Title),
    }
}

impl Scope {
    fn boundary<H>(self, e: &Open<H>) -> bool {
        match self {
            Scope::Default => default_boundary(e),
            Scope::ListItem => default_boundary(e) || e.html(Tag::Ol) || e.html(Tag::Ul),
            Scope::Button => default_boundary(e) || e.html(Tag::Button),
            Scope::Table => e.html(Tag::Html) || e.html(Tag::Table) || e.html(Tag::Template),
        }
    }
}

pub(crate) fn is_special<H>(e: &Open<H>) -> bool {
    use Tag::*;
    match e.ns {
        Namespace::Html => matches!(
            e.name.tag,
            Address
                | Applet
                | Area
                | Article
                | Aside
                | Base
                | Basefont
                | Bgsound
                | Blockquote
                | Body
                | Br
                | Button
                | Caption
                | Center
                | Col
                | Colgroup
                | Dd
                | Details
                | Dir
                | Div
                | Dl
                | Dt
                | Embed
                | Fieldset
                | Figcaption
                | Figure
                | Footer
                | Form
                | Frame
                | Frameset
                | H1
                | H2
                | H3
                | H4
                | H5
                | H6
                | Head
                | Header
                | Hgroup
                | Hr
                | Html
                | Iframe
                | Img
                | Input
                | Keygen
                | Li
                | Link
                | Listing
                | Main
                | Marquee
                | Menu
                | Meta
                | Nav
                | Noembed
                | Noframes
                | Noscript
                | Object
                | Ol
                | P
                | Param
                | Plaintext
                | Pre
                | Script
                | Search
                | Section
                | Select
                | Source
                | Style
                | Summary
                | Table
                | Tbody
                | Td
                | Template
                | Textarea
                | Tfoot
                | Th
                | Thead
                | Title
                | Tr
                | Track
                | Ul
                | Wbr
                | Xmp
        ),
        _ => default_boundary(e),
    }
}

fn implied<H>(e: &Open<H>) -> bool {
    e.ns == Namespace::Html
        && matches!(
            e.name.tag,
            Tag::Dd
                | Tag::Dt
                | Tag::Li
                | Tag::Optgroup
                | Tag::Option
                | Tag::P
                | Tag::Rb
                | Tag::Rp
                | Tag::Rt
                | Tag::Rtc
        )
}

pub(crate) fn is_heading<H>(e: &Open<H>) -> bool {
    e.ns == Namespace::Html
        && matches!(
            e.name.tag,
            Tag::H1 | Tag::H2 | Tag::H3 | Tag::H4 | Tag::H5 | Tag::H6
        )
}

impl<S: TreeSink> TreeBuilder<S> {
    pub(crate) fn in_scope_by(&self, scope: Scope, f: impl Fn(&Open<S::Handle>) -> bool) -> bool {
        for e in self.open.iter().rev() {
            step!(1);
            if f(e) {
                return true;
            }
            if scope.boundary(e) {
                return false;
            }
        }
        false
    }

    /// An HTML element of this name is in scope.
    pub(crate) fn in_scope(&self, scope: Scope, name: Name) -> bool {
        if name.tag != Tag::Other && self.open.html_open(name.tag) == 0 {
            return false;
        }
        self.in_scope_by(scope, |e| e.ns == Namespace::Html && e.name == name)
    }

    pub(crate) fn has_in_scope(&self, tag: Tag) -> bool {
        self.in_scope(Scope::Default, Name::known(tag))
    }

    pub(crate) fn current_is(&self, tag: Tag) -> bool {
        self.open.last().is_some_and(|e| e.html(tag))
    }

    pub(crate) fn pop(&mut self) -> Open<S::Handle> {
        let e = self.open.pop().expect("an element to pop");
        self.sink.children_finished(e.node);
        e
    }

    pub(crate) fn remove_open(&mut self, i: usize) {
        let e = self.open.remove(i);
        self.sink.children_finished(e.node);
    }

    pub(crate) fn pop_until(&mut self, f: impl Fn(&Open<S::Handle>) -> bool) {
        while !self.open.is_empty() {
            if f(&self.pop()) {
                return;
            }
        }
    }

    pub(crate) fn pop_until_html(&mut self, name: Name) {
        self.pop_until(|e| e.ns == Namespace::Html && e.name == name);
    }

    /// Generate implied end tags, except for HTML elements named `except`.
    pub(crate) fn generate_implied(&mut self, except: Option<Tag>) {
        while let Some(e) = self.open.last() {
            if !implied(e) || except.is_some_and(|t| e.html(t)) {
                return;
            }
            self.pop();
        }
    }

    pub(crate) fn generate_implied_thoroughly(&mut self) {
        while let Some(e) = self.open.last() {
            let table_part = e.ns == Namespace::Html
                && matches!(
                    e.name.tag,
                    Tag::Caption
                        | Tag::Colgroup
                        | Tag::Tbody
                        | Tag::Td
                        | Tag::Tfoot
                        | Tag::Th
                        | Tag::Thead
                        | Tag::Tr
                );
            if !implied(e) && !table_part {
                return;
            }
            self.pop();
        }
    }

    pub(crate) fn close_p(&mut self) {
        self.generate_implied(Some(Tag::P));
        self.pop_until(|e| e.html(Tag::P));
    }

    pub(crate) fn close_p_in_button_scope(&mut self) {
        if self.in_scope(Scope::Button, Name::known(Tag::P)) {
            self.close_p();
        }
    }

    /// Pop until the current node is one of the given HTML elements.
    pub(crate) fn clear_back_to(&mut self, tags: &[Tag]) {
        while let Some(e) = self.open.last() {
            if e.ns == Namespace::Html && tags.contains(&e.name.tag) {
                return;
            }
            self.pop();
        }
    }

    pub(crate) fn reset_mode(&mut self) {
        for i in (0..self.open.len()).rev() {
            step!(1);
            let last = i == 0;
            let node = match self.context {
                Some(c) if last => c,
                _ => self.open[i],
            };
            if node.ns != Namespace::Html {
                if last {
                    break;
                }
                continue;
            }
            self.mode = match node.name.tag {
                Tag::Td | Tag::Th if !last => Mode::InCell,
                Tag::Tr => Mode::InRow,
                Tag::Tbody | Tag::Thead | Tag::Tfoot => Mode::InTableBody,
                Tag::Caption => Mode::InCaption,
                Tag::Colgroup => Mode::InColumnGroup,
                Tag::Table => Mode::InTable,
                Tag::Template => *self.template_modes.last().expect("a template mode"),
                Tag::Head if !last => Mode::InHead,
                Tag::Body => Mode::InBody,
                Tag::Frameset => Mode::InFrameset,
                Tag::Html if self.head.is_none() => Mode::BeforeHead,
                Tag::Html => Mode::AfterHead,
                _ if last => break,
                _ => continue,
            };
            return;
        }
        self.mode = Mode::InBody;
    }
}
