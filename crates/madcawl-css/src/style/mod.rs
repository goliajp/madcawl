//! The cascade and computed values over a `madcawl_dom::Dom`.
//!
//! A `Cascade` takes stylesheets in order, each with its origin. Adding
//! one evaluates its `@media` and `@supports` conditions against the
//! device, finds its cascade layers, and flattens its style rules to
//! one entry per selector; declaration blocks stay unparsed until an
//! element matches them. `Cascade::compute` walks the document in tree
//! order and gives every element its computed values (`Styles`), in
//! groups shared until written (`computed.rs`); an element whose chain
//! of what selectors see matches one styled before takes its values by
//! `PathKey` (`pathkey.rs`). `Cascade::restyle` brings the styles up to
//! date after the document changes (`restyle.rs`).
//!
//! `style_document` does the whole thing for a parsed page: the user
//! agent stylesheet, then the page's `<style>` elements and the
//! stylesheets its `<link>` elements name, as the caller loads them.

/// A keyword-valued type: its variants, how each is written, and the
/// one a lowercase keyword names.
macro_rules! keywords {
    ($(#[$m:meta])* $ty:ident { $($v:ident = $s:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $ty {
            $($v),*
        }

        impl $ty {
            pub fn css(self) -> &'static str {
                match self {
                    $($ty::$v => $s),*
                }
            }

            /// The one an ASCII-lowercase keyword names.
            pub fn from_keyword(k: &str) -> Option<$ty> {
                match k {
                    $($s => Some($ty::$v),)*
                    _ => None,
                }
            }
        }
    };
}

mod ancestors;
mod before_after;
mod buckets;
mod calc;
mod caseless;
mod colors;
mod compute;
mod computed;
mod cssom;
mod custom;
mod decls;
mod device;
mod features;
mod fonts;
mod generated;
pub mod grid;
mod grid_parse;
mod hints;
mod initial;
mod keys;
mod kw;
mod layers;
mod length;
mod logical;
mod longhand;
mod matching;
mod media;
mod nth;
mod order;
mod pathkey;
mod prepared;
mod props;
mod pseudo;
mod render;
mod resolve;
mod resolve_box;
mod resolve_grid;
mod restyle;
mod roots;
mod rules;
mod shared;
mod shorthand;
mod sources;
mod states;
mod styles;
mod supports;
mod vals;
mod values;
mod var;

use std::cell::OnceCell;
use std::sync::{Arc, OnceLock};
use std::sync::atomic::{AtomicU64, Ordering};

use madcawl_dom::Dom;
use madcawl_str::HeldAtoms;

use crate::Span;
use crate::sheet::Stylesheet;
use crate::tokenizer::{Kind, Token, Tokenizer};

use prepared::Prepared;

pub use computed::{
    BoxModel, ComputedStyle, CssColor, Font, LineHeight, Paint, Place, Rare, Size, Text, VAlign,
};
pub use device::Device;
pub use features::Tier;
pub use generated::{AspectRatio, Content, ContentItem};
pub use kw::*;
pub use length::{Form, Lp, to_fixed};
pub use media::matches_media;
pub use props::Prop;
pub use restyle::{Change, Restyled};
pub use rules::Origin;
pub use shared::{Scope, SheetCache, SheetCacheStats, style_document_shared};
pub use sources::{SheetSource, sheet_sources};
pub use states::ElementStates;
pub use styles::{StyleStats, Styles};
pub use supports::supports;
pub use values::{ContentVisibility, Display, Float, Position, Visibility};

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
    /// Built from the rules on first use, for each quirks mode
    prepared: [OnceCell<Prepared>; 2],
    /// Unique to this cascade and its set of stylesheets: styles made
    /// under another one cannot be restyled incrementally
    generation: u64,
    path_key: bool,
    verify: bool,
    ancestor_filter: bool,
    buckets: bool,
}

fn is_quirks(dom: &Dom) -> bool {
    dom.quirks_mode() == madcawl_html::QuirksMode::Quirks
}

fn next_generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// How many selectors and declaration blocks the cascade holds, and how
/// many blocks an element has matched, which parses them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuleStats {
    pub selectors: usize,
    pub blocks: usize,
    pub parsed: usize,
}

impl<'a> Cascade<'a> {
    pub fn new(device: Device) -> Self {
        Cascade {
            rules: rules::Rules::new(),
            held: HeldAtoms::new(),
            device,
            prepared: Default::default(),
            generation: next_generation(),
            path_key: true,
            verify: cfg!(debug_assertions),
            ancestor_filter: true,
            buckets: true,
        }
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    /// Add a stylesheet after those already added. On the main thread,
    /// which owns the atom table.
    pub fn add(&mut self, sheet: &'a Stylesheet, origin: Origin) {
        self.rules.add(sheet, origin, &self.held, &self.device);
        self.prepared = Default::default();
        self.generation = next_generation();
    }

    /// Whether elements are looked up by `PathKey` (on by default); off,
    /// every element takes the full path.
    pub fn set_path_key(&mut self, on: bool) {
        self.path_key = on;
    }

    /// Whether every `PathKey` hit is computed again in full and
    /// compared (`StyleStats::mismatches`); on by default in debug
    /// builds.
    pub fn set_verify(&mut self, on: bool) {
        self.verify = on;
    }

    /// Whether rules that need an ancestor to have an id, class or name none has are left untried (on by
    /// default); off, every candidate rule is matched, which tests compare against.
    #[doc(hidden)]
    pub fn set_ancestor_filter(&mut self, on: bool) {
        self.ancestor_filter = on;
    }

    /// Whether an element is tried against the rules of its buckets only (on by default); off, against every
    /// rule, which tests compare against.
    #[doc(hidden)]
    pub fn set_buckets(&mut self, on: bool) {
        self.buckets = on;
    }

    pub fn rule_stats(&self) -> RuleStats {
        RuleStats {
            selectors: self.rules.entries.len(),
            blocks: self.rules.blocks.len(),
            parsed: self
                .rules
                .blocks
                .iter()
                .filter(|b| b.parsed.get().is_some())
                .count(),
        }
    }

    fn prepared(&self, dom: &Dom) -> &Prepared {
        let quirks = is_quirks(dom);
        self.prepared[usize::from(quirks)].get_or_init(|| Prepared::new(&self.rules, quirks))
    }

    fn run<'r>(&'r self, dom: &'r Dom, states: &'r ElementStates) -> compute::Run<'r, 'a> {
        let mut r = compute::Run::new(dom, &self.rules, self.prepared(dom), states, self.device);
        r.path_key = self.path_key;
        r.verify = self.verify;
        r.ancestors.off = !self.ancestor_filter;
        r.every_rule = !self.buckets;
        r
    }

    /// The computed values of every element of `dom`'s document.
    pub fn compute(&self, dom: &Dom) -> Styles {
        self.compute_with(dom, &ElementStates::new())
    }

    /// The same, the elements in the states `states` gives.
    pub fn compute_with(&self, dom: &Dom, states: &ElementStates) -> Styles {
        let mut out = Styles {
            generation: self.generation,
            quirks: is_quirks(dom),
            ..Styles::default()
        };
        out.memo.new_run();
        self.run(dom, states).all(&mut out);
        out
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
    shared::style_parsed_by(dom, device, load, &mut |t| Stylesheet::parse(t).ok().map(Arc::new))
}
