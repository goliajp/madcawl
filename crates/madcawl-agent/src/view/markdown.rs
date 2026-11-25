//! The Markdown view: headings, paragraphs, lists, tables, code blocks
//! with their language, links as `[text](ref)`; of the other elements
//! only the interactive ones carry a reference, as `[role: name](ref)`.

use madcawl_dom::{NodeId, NodeKind};
use madcawl_str::atom;

use super::md_text::{fenced, push_inline_code, push_md_text, shown_text};
use super::{Reader, View, finish};
use crate::html_table::is_html;
use crate::record::Record;
use crate::role::Role;
use crate::text::{attr_string, push_collapsed};

/// How deep the view reads; what is deeper is left out.
const MAX_DEPTH: u32 = 512;

pub(crate) struct Block {
    pub text: String,
    /// A table's rows, for folding
    pub table: Option<TableRows>,
    /// A list item: no blank line between it and the next item
    pub item: bool,
}

pub(crate) struct TableRows {
    /// The header row and the line under it
    pub head: String,
    pub body: Vec<String>,
    pub reference: String,
}

struct Md<'r, 'a> {
    r: &'r Reader<'a>,
    blocks: Vec<Block>,
    para: String,
    prefix: String,
    lists: Vec<bool>,
}

pub(crate) fn write(r: &Reader) -> View {
    let mut md = Md {
        r,
        blocks: Vec::new(),
        para: String::new(),
        prefix: String::new(),
        lists: Vec::new(),
    };
    md.walk(r.root(), 0);
    md.flush();
    let (text, folded) = super::fold_md::fit(md.blocks, r.opts.budget);
    finish(text, folded)
}

impl Md<'_, '_> {
    /// Ends the block being filled; a bullet or heading mark waiting for
    /// text stays for the next text when there is none yet.
    fn flush(&mut self) {
        let t = self.para.trim();
        if t.is_empty() {
            self.para.clear();
            return;
        }
        let text = format!("{}{}", self.prefix, t);
        let item = self.prefix.ends_with("- ") || self.prefix.ends_with("1. ");
        self.blocks.push(Block {
            text,
            table: None,
            item,
        });
        self.para.clear();
        self.prefix.clear();
    }

    fn inline(&mut self, s: &str) {
        let mut buf = String::new();
        push_md_text(&mut buf, s);
        if !self.para.is_empty() && !self.para.ends_with(' ') && s.starts_with(char::is_whitespace)
        {
            self.para.push(' ');
        }
        push_collapsed(&mut self.para, &buf);
        if s.ends_with(char::is_whitespace) && !self.para.ends_with(' ') {
            self.para.push(' ');
        }
    }

    fn walk(&mut self, n: NodeId, depth: u32) {
        let Some(rec) = self.r.record(n) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        if rec.role == Role::None && n != self.r.dom.document() {
            return;
        }
        let shown = self.r.writes(n);
        if self.r.dom.kind(n) == NodeKind::Text {
            if shown.is_some() {
                self.inline(self.r.name(&rec));
            }
            return;
        }
        if let Some(rec) = shown
            && self.special(n, &rec, depth)
        {
            return;
        }
        let block = self.block(n);
        if block {
            self.flush();
        }
        let list = matches!(rec.role, Role::List) && shown.is_some();
        if list {
            self.lists.push(is_html(self.r.dom, n, atom!("ol")));
        }
        let kids: Vec<NodeId> = self.r.dom.children(n).collect();
        for c in kids {
            self.walk(c, depth + 1);
        }
        if list {
            self.lists.pop();
        }
        if block {
            self.flush();
        }
    }

    fn block(&self, n: NodeId) -> bool {
        let cx = crate::cx::Cx {
            dom: self.r.dom,
            styles: self.r.inputs.styles,
            ids: &self.r.tree.ids,
            label_for: &self.r.tree.label_for,
        };
        if self.r.inputs.styles.is_some() {
            return cx.block_level(n);
        }
        self.r.record(n).is_some_and(|r| {
            matches!(
                r.role,
                Role::Paragraph | Role::Blockquote | Role::List | Role::Group | Role::Article
            )
        })
    }

    /// Writes the node in its own way; false to read it as a container.
    fn special(&mut self, n: NodeId, rec: &Record, depth: u32) -> bool {
        let name = self.r.name(rec).to_owned();
        match rec.role {
            Role::Heading => {
                self.flush();
                self.prefix = "#".repeat(rec.level.clamp(1, 6) as usize) + " ";
                self.inline(&name);
                self.flush();
                self.prefix.clear();
            }
            Role::ListItem => {
                self.flush();
                let indent = self.lists.len().saturating_sub(1);
                let bullet = if self.lists.last() == Some(&true) {
                    "1. "
                } else {
                    "- "
                };
                self.prefix = "  ".repeat(indent) + bullet;
                let kids: Vec<NodeId> = self.r.dom.children(n).collect();
                for c in kids {
                    self.walk(c, depth + 1);
                }
                self.flush();
                self.prefix.clear();
            }
            Role::Table | Role::Grid | Role::TreeGrid => {
                self.flush();
                let b = super::table::table(self.r, n);
                self.prefix.clear();
                self.blocks.push(b);
            }
            Role::Link => {
                self.para_link(&name, n, rec);
            }
            Role::Image => {
                if !name.is_empty() {
                    let mut s = String::from("![");
                    push_md_text(&mut s, &name);
                    s.push_str("] ");
                    self.para.push_str(&s);
                }
            }
            Role::Separator => {
                self.flush();
                self.prefix.clear();
                self.blocks.push(Block {
                    text: "---".to_owned(),
                    table: None,
                    item: false,
                });
            }
            Role::Code if is_html(self.r.dom, n, atom!("code")) => {
                let t = shown_text(self.r, n);
                push_inline_code(&mut self.para, t.trim());
                self.para.push(' ');
            }
            _ if is_html(self.r.dom, n, atom!("pre")) => self.pre(n),
            r if r.interactive() => {
                let label = format!("{}: {}", r.name(), name);
                self.para_link(&label, n, rec);
            }
            _ => return false,
        }
        true
    }

    fn para_link(&mut self, text: &str, n: NodeId, rec: &Record) {
        let mut s = String::from("[");
        push_md_text(&mut s, text);
        s.push_str("](");
        self.r.reference(n, rec).write(&mut s);
        s.push_str(") ");
        if !self.para.is_empty() && !self.para.ends_with(' ') {
            self.para.push(' ');
        }
        self.para.push_str(&s);
    }

    fn pre(&mut self, n: NodeId) {
        self.flush();
        self.prefix.clear();
        let dom = self.r.dom;
        let lang = std::iter::once(n)
            .chain(dom.children(n).filter(|&c| is_html(dom, c, atom!("code"))))
            .filter_map(|e| attr_string(dom, e, atom!("class")))
            .flat_map(|c| {
                c.split_whitespace()
                    .filter_map(|w| w.strip_prefix("language-").or(w.strip_prefix("lang-")))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .next()
            .unwrap_or_default();
        let text = fenced(&lang, shown_text(self.r, n).trim_end_matches('\n'));
        self.blocks.push(Block {
            text,
            table: None,
            item: false,
        });
    }
}
