//! The cascade and computed values over a `madcawl_dom::Dom`.
//!
//! A `Cascade` takes stylesheets in order, each with its origin. Adding
//! one evaluates its `@media` and `@supports` conditions against the
//! device, finds its cascade layers, and flattens its style rules to
//! one entry per selector; declaration blocks stay unparsed until an
//! element matches them. `Cascade::compute` walks the document in tree
//! order and gives every element its computed values (`Styles`).
//!
//! `style_document` does the whole thing for a parsed page: the user
//! agent stylesheet, then the page's `<style>` elements and the
//! stylesheets its `<link>` elements name, as the caller loads them.

mod buckets;
mod compute;
mod matching;
mod media;
mod props;
mod pseudo;
mod rules;
mod sources;
mod supports;
mod values;

use std::sync::OnceLock;

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_str::HeldAtoms;

use crate::Span;
use crate::sheet::Stylesheet;
use crate::tokenizer::{Kind, Token, Tokenizer};

pub use media::{Device, matches_media};
pub use props::Prop;
pub use rules::Origin;
pub use sources::{SheetSource, sheet_sources};
pub use supports::supports;
pub use values::{ComputedStyle, ContentVisibility, Display, Float, Position, Visibility};

/// The tokens of `src[span]` but whitespace.
pub(crate) fn tokens(src: &str, span: Span) -> Vec<Token> {
    let mut t = Tokenizer::in_span(src, span);
    std::iter::from_fn(|| Some(t.next_token()))
        .take_while(|t| t.kind != Kind::Eof)
        .filter(|t| t.kind != Kind::Whitespace)
        .collect()
}

/// The HTML standard's rendering rules that decide what is displayed,
/// as WebKit applies them.
pub const UA_CSS: &str = include_str!("ua.css");

/// The user agent stylesheet, parsed once.
pub fn ua_sheet() -> &'static Stylesheet {
    static UA: OnceLock<Stylesheet> = OnceLock::new();
    UA.get_or_init(|| Stylesheet::parse(UA_CSS.to_owned()).expect("a small stylesheet"))
}

pub struct Cascade<'a> {
    rules: rules::Rules<'a>,
    held: HeldAtoms,
    device: Device,
}

impl<'a> Cascade<'a> {
    pub fn new(device: Device) -> Self {
        Cascade {
            rules: rules::Rules::new(),
            held: HeldAtoms::new(),
            device,
        }
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    /// Add a stylesheet after those already added. On the main thread,
    /// which owns the atom table.
    pub fn add(&mut self, sheet: &'a Stylesheet, origin: Origin) {
        self.rules.add(sheet, origin, &self.held, &self.device);
    }

    /// The computed values of every element of `dom`'s document.
    pub fn compute(&self, dom: &Dom) -> Styles {
        let mut out = Styles::default();
        compute::Run::new(dom, &self.rules).all(&mut out);
        out
    }
}

/// The element or one of its ancestors is `display: none`.
const NOT_RENDERED: u8 = 1;
/// An ancestor is `content-visibility: hidden`: its contents are skipped.
const CONTENTS_SKIPPED: u8 = 2;

/// The computed values of a document's elements, by node.
#[derive(Default)]
pub struct Styles {
    /// By node index, 1 + the index in `table`; 0 for a node that has none
    of: Vec<u32>,
    flags: Vec<u8>,
    table: Vec<ComputedStyle>,
}

impl Styles {
    /// The computed values of an element in the document.
    pub fn get(&self, n: NodeId) -> Option<&ComputedStyle> {
        let k = *self.of.get(n.index() as usize)?;
        (k != 0).then(|| &self.table[k as usize - 1])
    }

    /// How many different sets of computed values the elements have.
    pub fn distinct(&self) -> usize {
        self.table.len()
    }

    fn flags_of(&self, dom: &Dom, n: NodeId) -> Option<(u8, &ComputedStyle)> {
        let el = match dom.kind(n) {
            NodeKind::Element => n,
            _ => dom
                .parent(n)
                .filter(|&p| dom.kind(p) == NodeKind::Element)?,
        };
        let s = self.get(el)?;
        let mut f = self.flags[el.index() as usize];
        if el != n && s.content_visibility == ContentVisibility::Hidden {
            f |= CONTENTS_SKIPPED;
        }
        Some((f, s))
    }

    /// Whether the node makes boxes: an element or text in the document
    /// with no `display: none` on it or above it and no
    /// `content-visibility: hidden` above it. A text node goes by its
    /// parent element.
    pub fn is_rendered(&self, dom: &Dom, n: NodeId) -> bool {
        self.flags_of(dom, n).is_some_and(|(f, _)| f == 0)
    }

    /// Rendered, and `visibility: visible`.
    pub fn is_visible(&self, dom: &Dom, n: NodeId) -> bool {
        self.flags_of(dom, n)
            .is_some_and(|(f, s)| f == 0 && s.visibility == Visibility::Visible)
    }
}

/// The styles of a parsed page: the user agent stylesheet, then each of
/// the page's stylesheets whose `media` matches, in tree order. `load`
/// gives the text of a `<link>`ed stylesheet from its `href` as
/// written, or `None` when there is none.
pub fn style_document(
    dom: &Dom,
    device: Device,
    load: &mut dyn FnMut(&str) -> Option<String>,
) -> Styles {
    let mut sheets = Vec::new();
    for s in sheet_sources(dom) {
        let (text, media) = match s {
            SheetSource::Inline { text, media } => (Some(text), media),
            SheetSource::Link { href, media } => (load(&href), media),
        };
        let applies = media
            .as_deref()
            .is_none_or(|m| matches_media(m, Span::new(0, m.len()), &device));
        if let Some(text) = text.filter(|_| applies)
            && let Ok(sheet) = Stylesheet::parse(text)
        {
            sheets.push(sheet);
        }
    }
    let mut c = Cascade::new(device);
    c.add(ua_sheet(), Origin::UserAgent);
    for s in &sheets {
        c.add(s, Origin::Author);
    }
    c.compute(dom)
}
