//! A styled page read back as a list of items, each one line (a
//! preformatted block is several): landmarks, headings, blocks of text
//! with their links inline, list items, tables row by row, and form
//! controls. Nothing that is not shown (`Shown`) is read. Every item
//! names the node it came from, so a later action can target it.

mod blocks;
mod forms;
mod roles;
mod table;
mod text;
mod write;

use std::collections::HashMap;

use madcawl_css::style::Display;
use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_html::Namespace;
use madcawl_str::{Atom, atom};

use crate::Shown;
use text::{Line, heading_level, with_str};

pub use blocks::svg_title;
pub use text::{parse_reference, reference};
pub use write::write_text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A landmark or another region: `navigation`, `main`, `form`...
    Region(&'static str),
    Heading(u8),
    Text,
    ListItem,
    /// Rows by columns, and the caption
    Table,
    Row,
    /// A row of header cells only
    HeadRow,
    Pre,
    /// A form control or a widget, by role
    Control(&'static str),
    /// An option of a select
    Choice,
}

pub struct Item {
    pub depth: u16,
    pub kind: Kind,
    pub node: NodeId,
    pub text: String,
}

pub struct Read {
    pub title: Option<String>,
    /// The `<title>` element the title came from
    pub title_node: Option<NodeId>,
    pub items: Vec<Item>,
    /// Every node something in `items` was read from
    pub used: Vec<NodeId>,
}

pub fn read(shown: &Shown) -> Read {
    let mut r = Reader::new(shown);
    let doc = r.dom.document();
    r.walk(doc, 0, doc);
    r.flush();
    let title_node = r.dom.descendants(doc).find(|&n| {
        r.dom.local_name(n) == Some(atom!("title")) && r.dom.namespace(n) == Some(Namespace::Html)
    });
    let title = title_node.map(|t| {
        let mut line = Line::default();
        for c in r.dom.children(t) {
            if let Some(s) = r.dom.text(c).filter(|_| r.dom.kind(c) == NodeKind::Text) {
                with_str(s, |s| line.text(s));
            }
        }
        line.s
    });
    Read {
        title,
        title_node,
        items: r.items,
        used: r.used,
    }
}

/// The block being filled with inline content.
struct Open {
    depth: u16,
    kind: Kind,
    node: NodeId,
    line: Line,
}

pub(crate) struct Reader<'a> {
    dom: &'a Dom,
    shown: &'a Shown<'a>,
    items: Vec<Item>,
    used: Vec<NodeId>,
    open: Option<Open>,
    /// What the next block of inline content is, when not plain text
    next: Option<(Kind, NodeId, u16)>,
    ids: Option<HashMap<Atom, NodeId>>,
    labels: Option<HashMap<Atom, NodeId>>,
    /// The flat text of `aria-labelledby` targets, by target
    flattened: HashMap<NodeId, String>,
}

impl<'a> Reader<'a> {
    fn new(shown: &'a Shown<'a>) -> Self {
        Reader {
            dom: shown.dom,
            shown,
            items: Vec::new(),
            used: Vec::new(),
            open: None,
            next: None,
            ids: None,
            labels: None,
            flattened: HashMap::new(),
        }
    }

    fn flush(&mut self) {
        if let Some(o) = self.open.take()
            && !o.line.s.is_empty()
        {
            self.items.push(Item {
                depth: o.depth,
                kind: o.kind,
                node: o.node,
                text: o.line.s,
            });
        }
    }

    fn push(&mut self, depth: u16, kind: Kind, node: NodeId, text: String) {
        self.flush();
        self.used.push(node);
        self.items.push(Item {
            depth,
            kind,
            node,
            text,
        });
    }

    /// The line inline content goes on, opened in `block` when none is.
    fn line(&mut self, depth: u16, block: NodeId) -> &mut Line {
        if self.open.is_none() {
            let (kind, node, depth) = self.next.take().unwrap_or((Kind::Text, block, depth));
            self.used.push(node);
            self.open = Some(Open {
                depth,
                kind,
                node,
                line: Line::default(),
            });
        }
        &mut self.open.as_mut().expect("opened above").line
    }

    fn block_level(&self, el: NodeId) -> bool {
        use Display::*;
        let d = self.shown.styles.get(el).map_or(Inline, |s| s.display);
        !matches!(
            d,
            Inline
                | InlineBlock
                | InlineFlex
                | InlineGrid
                | InlineTable
                | InlineListItem
                | Contents
                | Ruby
                | RubyText
                | WebkitInlineBox
        )
    }

    fn walk(&mut self, n: NodeId, depth: u16, block: NodeId) {
        for c in self.dom.children(n) {
            match self.dom.kind(c) {
                NodeKind::Text if self.shown.shown(c) => {
                    let Some(t) = self.dom.text(c) else { continue };
                    let line = self.line(depth, block);
                    with_str(t, |s| line.text(s));
                    self.used.push(c);
                }
                NodeKind::Element if self.shown.rendered(c) => self.element(c, depth, block),
                _ => {}
            }
        }
    }

    fn element(&mut self, el: NodeId, depth: u16, block: NodeId) {
        if !self.shown.shown(el) {
            // visibility: hidden; what is below may be visible again
            let b = self.block_level(el);
            if b {
                self.flush();
            }
            self.walk(el, depth, block);
            if b {
                self.flush();
            }
            return;
        }
        if let Some(role) = self.attr(el, atom!("role"))
            && self.role(el, &role, depth)
        {
            return;
        }
        let html = self.dom.namespace(el) == Some(Namespace::Html);
        let name = self.dom.local_name(el).filter(|_| html);
        if name.is_some_and(|n| self.html_element(el, n, depth, block)) {
            return;
        }
        if self.block_level(el) {
            self.flush();
            self.walk(el, depth, el);
            self.flush();
        } else {
            self.walk(el, depth, block);
        }
    }

    /// Reads the HTML elements that are more than a block or inline
    /// content; false for the rest.
    fn html_element(&mut self, el: NodeId, name: Atom, depth: u16, block: NodeId) -> bool {
        match name {
            n if n == atom!("a") && self.dom.attribute(el, atom!("href")).is_some() => {
                self.link(el, depth, block)
            }
            n if n == atom!("br") => self.line(depth, block).space(),
            n if n == atom!("img") => {
                if let Some(alt) = self.attr(el, atom!("alt")) {
                    self.used.push(el);
                    self.line(depth, block)
                        .raw(&format!("![{}]", alt.replace(']', "\\]")));
                }
            }
            n if heading_level(Some(n)).is_some() => {
                let level = heading_level(Some(n)).expect("a heading");
                self.within(el, depth, Kind::Heading(level));
            }
            n if n == atom!("li") => self.within(el, depth, Kind::ListItem),
            n if n == atom!("table") => return self.table(el, depth),
            n if [
                atom!("pre"),
                atom!("listing"),
                atom!("plaintext"),
                atom!("xmp"),
            ]
            .contains(&n) =>
            {
                self.pre(el, depth)
            }
            n if [
                atom!("input"),
                atom!("select"),
                atom!("textarea"),
                atom!("button"),
            ]
            .contains(&n) =>
            {
                self.control(el, n, depth)
            }
            n => match self.html_region(el, n) {
                Some(role) => self.region(el, depth, role),
                None => return false,
            },
        }
        true
    }

    /// An element whose inline content is one item of `kind`; what is
    /// below it nests one level deeper.
    fn within(&mut self, el: NodeId, depth: u16, kind: Kind) {
        self.flush();
        self.next = Some((kind, el, depth));
        self.walk(el, depth + 1, el);
        self.flush();
        self.next = None;
    }
}
