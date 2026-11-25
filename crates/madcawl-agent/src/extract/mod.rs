//! Extraction: structured records straight from the semantic tree, so
//! an agent does not have to pick data out of a view's text. A table
//! becomes its rows keyed by the header, merged cells expanded; a
//! repeated structure (list items, cards, search results) becomes one
//! record an item; and every link, image and form field of a part of
//! the page is listed. Every record carries the reference of the node
//! it came from. What is not shown is left out unless asked for, and a
//! password's value is never given.

mod grid;
mod inventory;
mod list;
mod table;
mod write;

use std::borrow::Cow;
use std::rc::Rc;

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_str::{Atom, atom};

use crate::html_table::is_html;
use crate::tree::{Inputs, SemTree};
use crate::view::{Reader, Scope, View, ViewOpts};

/// What to extract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum What {
    /// The rows of the table the target names, or with no target a list
    /// of the tables in scope
    Table,
    /// The items of the repeated structure the target is in, or with no
    /// target the most prominent one in scope
    List,
    Links,
    Images,
    /// Every form control, with the form it belongs to
    Forms,
}

impl What {
    pub const NAMES: [&'static str; 5] = ["table", "list", "links", "images", "forms"];

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }

    pub fn from_name(s: &str) -> Option<What> {
        let all = [
            What::Table,
            What::List,
            What::Links,
            What::Images,
            What::Forms,
        ];
        all.into_iter().find(|w| w.name() == s.trim())
    }
}

/// How the records are written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    /// One JSON object a line
    #[default]
    JsonLines,
    Markdown,
}

#[derive(Clone, Copy, Debug)]
pub struct ExtractOpts {
    pub what: What,
    /// The table, an item of the list ("the same kind as this"), or the
    /// subtree links, images and forms are taken from
    pub target: Option<NodeId>,
    /// Where links, images, forms and an untargeted list are looked for
    pub scope: Scope,
    /// Records to skip, from an earlier reply's cursor
    pub offset: usize,
    /// At most this many estimated tokens of records
    pub budget: Option<usize>,
    pub include_hidden: bool,
    pub format: Format,
}

impl ExtractOpts {
    pub fn new(what: What) -> Self {
        ExtractOpts {
            what,
            target: None,
            scope: Scope::Page,
            offset: 0,
            budget: None,
            include_hidden: false,
            format: Format::JsonLines,
        }
    }
}

/// A field's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Flag(bool),
    List(Vec<String>),
}

impl Value {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }
}

/// One record: the node it came from, its reference, its fields in
/// order.
#[derive(Clone, Debug)]
pub struct Row {
    pub node: NodeId,
    pub reference: String,
    pub fields: Vec<(Rc<str>, Value)>,
}

impl Row {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields
            .iter()
            .find(|(k, _)| &**k == key)
            .map(|(_, v)| v)
    }
}

/// What an extraction found.
#[derive(Clone, Debug)]
pub struct Records {
    pub what: What,
    /// The reference of the table or of the node holding the items
    pub source: Option<String>,
    /// The table's caption or name, or how the items were recognised
    pub label: String,
    /// The keys in the order a table writes them
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    /// What was left out and why, when something was
    pub note: String,
}

impl Records {
    fn new(what: What) -> Self {
        Records {
            what,
            source: None,
            label: String::new(),
            columns: Vec::new(),
            rows: Vec::new(),
            note: String::new(),
        }
    }
}

impl SemTree {
    /// The records `opts` asks for, every one of them.
    pub fn extract(&self, dom: &Dom, inputs: Inputs, opts: &ExtractOpts) -> Records {
        let r = self.extract_reader(dom, inputs, opts);
        match opts.what {
            What::Table => match opts.target {
                Some(t) => table::rows(&r, t),
                None => table::index(&r),
            },
            What::List => list::items(&r, opts.target),
            What::Links => inventory::links(&r),
            What::Images => inventory::images(&r),
            What::Forms => inventory::forms(&r),
        }
    }

    /// The records written in the format asked for, from the offset and
    /// within the budget, after a line saying what they are and where
    /// the next ones start.
    pub fn extract_view(&self, dom: &Dom, inputs: Inputs, opts: &ExtractOpts) -> View {
        if let (What::Table, Some(t)) = (opts.what, opts.target) {
            // only the records the reply holds are made
            let r = self.extract_reader(dom, inputs, opts);
            if let Some(t) = table::Table::of(&r, t) {
                return write::write(&t.head, t.len(), opts, |i| Cow::Owned(t.row(&r, i)));
            }
        }
        let recs = self.extract(dom, inputs, opts);
        let total = recs.rows.len();
        write::write(&recs, total, opts, |i| Cow::Borrowed(&recs.rows[i]))
    }

    fn extract_reader<'a>(
        &'a self,
        dom: &'a Dom,
        inputs: Inputs<'a>,
        opts: &ExtractOpts,
    ) -> Reader<'a> {
        let scope = match (opts.what, opts.target) {
            (What::Links | What::Images | What::Forms, Some(t)) => Scope::Subtree(t),
            _ => opts.scope,
        };
        let vo = ViewOpts {
            scope,
            budget: None,
            include_hidden: opts.include_hidden,
        };
        Reader::new(self, dom, inputs, vo)
    }
}

/// The elements whose text runs on with the text around them.
pub(crate) const INLINE: [Atom; 27] = [
    atom!("a"),
    atom!("abbr"),
    atom!("b"),
    atom!("bdi"),
    atom!("bdo"),
    atom!("cite"),
    atom!("code"),
    atom!("data"),
    atom!("dfn"),
    atom!("em"),
    atom!("font"),
    atom!("i"),
    atom!("kbd"),
    atom!("label"),
    atom!("mark"),
    atom!("q"),
    atom!("s"),
    atom!("samp"),
    atom!("small"),
    atom!("span"),
    atom!("strong"),
    atom!("sub"),
    atom!("sup"),
    atom!("time"),
    atom!("u"),
    atom!("var"),
    atom!("wbr"),
];

/// The visible text below `n`, collapsed, in tree order; a line break
/// or the start of a block is a space.
pub(crate) fn shown_text(r: &Reader, n: NodeId) -> String {
    let mut out = String::new();
    for d in r.dom.descendants(n) {
        match r.dom.kind(d) {
            NodeKind::Text if r.writes(d).is_some() => {
                if let Some(t) = r.dom.text(d) {
                    crate::text::push_collapsed(&mut out, &crate::text::string(t));
                }
            }
            NodeKind::Element if !r.dom.local_name(d).is_some_and(|a| INLINE.contains(&a)) => {
                crate::text::push_collapsed(&mut out, " ");
            }
            _ => {}
        }
    }
    // a no-break space is not collapsed, but at either end it is padding
    match out.trim_matches(char::is_whitespace) {
        t if t.len() == out.len() => out,
        t => t.to_owned(),
    }
}

pub(crate) fn attr(dom: &Dom, n: NodeId, a: Atom) -> Option<String> {
    crate::text::attr_string(dom, n, a)
}

/// The reference of any node in the tree, shown or not.
pub(crate) fn reference(r: &Reader, n: NodeId) -> String {
    r.record(n)
        .map(|rec| r.reference(n, &rec).text())
        .unwrap_or_default()
}

/// The nearest landmark around `n`: its role, and its name when it has
/// one.
pub(crate) fn landmark(r: &Reader, n: NodeId) -> String {
    let mut up = r.dom.parent(n);
    while let Some(p) = up {
        if let Some(rec) = r.record(p).filter(|rec| rec.role.landmark()) {
            let name = r.name(&rec);
            return match name.is_empty() {
                true => rec.role.name().to_owned(),
                false => format!("{} {name}", rec.role.name()),
            };
        }
        up = r.dom.parent(p);
    }
    String::new()
}

/// An HTML `form` element.
pub(crate) fn is_form(dom: &Dom, n: NodeId) -> bool {
    is_html(dom, n, atom!("form"))
}
