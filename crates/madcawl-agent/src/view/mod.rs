//! The views an agent reads, written straight from the semantic tree:
//! the outline, the snapshot, Markdown, and find. Each takes a scope, an
//! optional budget of estimated tokens it folds itself into, and
//! whether to include what is not shown.

mod find;
mod fold;
mod fold_md;
mod markdown;
mod md_text;
mod outline;
mod snapshot;
mod table;

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_str::atom;

use crate::geom::Geometry;
use crate::record::{Record, VISIBLE};
use crate::refs::Ref;
use crate::role::Role;
use crate::tree::{Inputs, SemTree};

pub use find::Query;
pub(crate) use snapshot::node_line;

/// Which part of the page a view covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Page,
    /// What is at least partly in the viewport, and what holds it
    Viewport,
    /// A node and what is below it
    Subtree(NodeId),
    /// The `main` or `article` landmark, else the block with the most
    /// text
    Main,
}

#[derive(Clone, Copy, Debug)]
pub struct ViewOpts {
    pub scope: Scope,
    /// At most this many estimated tokens; folded by structure past it
    pub budget: Option<usize>,
    /// Write what is not shown too, each such node marked `hidden`
    pub include_hidden: bool,
}

impl Default for ViewOpts {
    fn default() -> Self {
        ViewOpts {
            scope: Scope::Page,
            budget: None,
            include_hidden: false,
        }
    }
}

/// A view as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub text: String,
    /// The estimated tokens of `text` (ASCII bytes / 4, other characters 1)
    pub tokens: usize,
    /// The lines or items folded away to fit the budget
    pub folded: usize,
}

pub(crate) struct Reader<'a> {
    pub tree: &'a SemTree,
    pub dom: &'a Dom,
    pub inputs: Inputs<'a>,
    pub opts: ViewOpts,
}

impl<'a> Reader<'a> {
    pub fn new(tree: &'a SemTree, dom: &'a Dom, inputs: Inputs<'a>, opts: ViewOpts) -> Self {
        Reader {
            tree,
            dom,
            inputs,
            opts,
        }
    }

    pub fn geometry(&self) -> Option<&'a Geometry> {
        self.inputs.geometry
    }

    pub fn record(&self, n: NodeId) -> Option<Record> {
        self.tree.record(n).copied()
    }

    /// The node's own content may be written: shown (or hidden ones
    /// asked for), and in the viewport for that scope.
    pub fn writes(&self, n: NodeId) -> Option<Record> {
        let r = self.record(n)?;
        if !r.has(VISIBLE) && !self.opts.include_hidden {
            return None;
        }
        if self.opts.scope == Scope::Viewport
            && self
                .geometry()
                .and_then(|g| g.in_viewport(n))
                .is_some_and(|v| !v)
        {
            return None;
        }
        Some(r)
    }

    pub fn reference(&self, n: NodeId, r: &Record) -> Ref {
        Ref {
            index: n.index(),
            slot_gen: r.generation,
            page_gen: self.tree.page_gen,
        }
    }

    pub fn push_ref(&self, out: &mut String, n: NodeId, r: &Record) {
        out.push('[');
        self.reference(n, r).write(out);
        out.push(']');
    }

    pub fn name(&self, r: &Record) -> &'a str {
        self.tree.name(r)
    }

    /// Where the view starts.
    pub fn root(&self) -> NodeId {
        match self.opts.scope {
            Scope::Subtree(n) => n,
            Scope::Main => self.main().unwrap_or(self.dom.document()),
            _ => self.dom.document(),
        }
    }

    fn main(&self) -> Option<NodeId> {
        let doc = self.dom.document();
        let landmark = self.dom.descendants(doc).find(|&n| {
            self.record(n)
                .is_some_and(|r| r.shown() && matches!(r.role, Role::Main))
        });
        landmark
            .or_else(|| {
                self.dom.descendants(doc).find(|&n| {
                    self.record(n)
                        .is_some_and(|r| r.shown() && r.role == Role::Article)
                })
            })
            .or_else(|| self.densest())
    }

    /// The element whose paragraphs hold the most text directly.
    fn densest(&self) -> Option<NodeId> {
        let mut best: Option<(usize, NodeId)> = None;
        let mut sums: std::collections::HashMap<NodeId, usize> = Default::default();
        for n in self.dom.descendants(self.dom.document()) {
            if !self
                .record(n)
                .is_some_and(|r| r.shown() && r.role == Role::Paragraph)
            {
                continue;
            }
            let len: usize = self
                .dom
                .children(n)
                .filter_map(|c| {
                    self.record(c)
                        .filter(|_| self.dom.kind(c) == NodeKind::Text)
                })
                .map(|c| self.name(&c).len())
                .sum();
            if let Some(p) = self.dom.parent(n) {
                let s = sums.entry(p).or_default();
                *s += len;
                if best.is_none_or(|(b, _)| *s > b) {
                    best = Some((*s, p));
                }
            }
        }
        best.map(|(_, n)| n)
    }

    /// The document's `<title>`, collapsed.
    pub fn title(&self) -> Option<String> {
        let doc = self.dom.document();
        let t = self
            .dom
            .descendants(doc)
            .find(|&n| crate::html_table::is_html(self.dom, n, atom!("title")))?;
        let s = crate::text::collapsed(&crate::text::text_content(self.dom, t));
        (!s.is_empty()).then_some(s)
    }
}

pub(crate) fn finish_text(text: String) -> View {
    finish(text, 0)
}

pub(crate) fn finish(text: String, folded: usize) -> View {
    View {
        tokens: crate::text::estimate_tokens(&text),
        text,
        folded,
    }
}

impl SemTree {
    pub fn outline(&self, dom: &Dom, inputs: Inputs, opts: ViewOpts) -> View {
        outline::write(&Reader::new(self, dom, inputs, opts))
    }

    pub fn snapshot(&self, dom: &Dom, inputs: Inputs, opts: ViewOpts) -> View {
        snapshot::write(&Reader::new(self, dom, inputs, opts))
    }

    pub fn markdown(&self, dom: &Dom, inputs: Inputs, opts: ViewOpts) -> View {
        markdown::write(&Reader::new(self, dom, inputs, opts))
    }

    pub fn find(&self, dom: &Dom, inputs: Inputs, opts: ViewOpts, q: &Query) -> View {
        find::write(&Reader::new(self, dom, inputs, opts), q)
    }
}
