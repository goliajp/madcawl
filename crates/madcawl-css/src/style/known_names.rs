//! The names of every property the cascade knows, as CSSOM lists the
//! supported CSS properties: the longhands, the flow-relative longhands,
//! the shorthands, and the older or prefixed names some of them have.

use std::sync::OnceLock;

use super::logical::{Logical, logical_name};
use super::props::{PROPS, Prop, What};
use super::shorthand::{self, Short};

/// An older or prefixed name, and the name it stands for.
pub(crate) const ALIASES: &[(&str, &str)] = &[
    ("-webkit-box-sizing", "box-sizing"),
    ("-webkit-flex-direction", "flex-direction"),
    ("-webkit-flex-wrap", "flex-wrap"),
    ("-webkit-flex-grow", "flex-grow"),
    ("-webkit-flex-shrink", "flex-shrink"),
    ("-webkit-flex-basis", "flex-basis"),
    ("-webkit-order", "order"),
    ("grid-row-gap", "row-gap"),
    ("grid-column-gap", "column-gap"),
    ("grid-gap", "gap"),
    ("-webkit-justify-content", "justify-content"),
    ("-webkit-align-items", "align-items"),
    ("-webkit-align-self", "align-self"),
    ("-webkit-align-content", "align-content"),
    ("-webkit-writing-mode", "writing-mode"),
    ("-webkit-flex", "flex"),
    ("-webkit-flex-flow", "flex-flow"),
    ("-webkit-margin-start", "margin-inline-start"),
    ("-webkit-margin-end", "margin-inline-end"),
    ("-webkit-margin-before", "margin-block-start"),
    ("-webkit-margin-after", "margin-block-end"),
    ("-webkit-padding-start", "padding-inline-start"),
    ("-webkit-padding-end", "padding-inline-end"),
    ("-webkit-padding-before", "padding-block-start"),
    ("-webkit-padding-after", "padding-block-end"),
    ("-webkit-transform", "transform"),
    ("-webkit-transform-origin", "transform-origin"),
    ("-webkit-transition", "transition"),
    ("-webkit-animation", "animation"),
];

pub(crate) const LOGICAL: &[&str] = &[
    "margin-block-start",
    "margin-block-end",
    "margin-inline-start",
    "margin-inline-end",
    "padding-block-start",
    "padding-block-end",
    "padding-inline-start",
    "padding-inline-end",
    "inset-block-start",
    "inset-block-end",
    "inset-inline-start",
    "inset-inline-end",
    "border-block-start-width",
    "border-block-end-width",
    "border-inline-start-width",
    "border-inline-end-width",
    "border-block-start-style",
    "border-block-end-style",
    "border-inline-start-style",
    "border-inline-end-style",
    "border-block-start-color",
    "border-block-end-color",
    "border-inline-start-color",
    "border-inline-end-color",
    "inline-size",
    "block-size",
    "min-inline-size",
    "min-block-size",
    "max-inline-size",
    "max-block-size",
];

/// The shorthands, by their own names.
pub(crate) const SHORTHANDS: &[&str] = &[
    "grid",
    "border",
    "grid-template",
    "grid-area",
    "font",
    "margin",
    "padding",
    "inset",
    "border-width",
    "border-style",
    "border-color",
    "border-block",
    "border-inline",
    "flex",
    "border-top",
    "border-right",
    "border-bottom",
    "border-left",
    "border-block-start",
    "border-block-end",
    "border-inline-start",
    "border-inline-end",
    "margin-block",
    "margin-inline",
    "padding-block",
    "padding-inline",
    "inset-block",
    "inset-inline",
    "border-block-width",
    "border-block-style",
    "border-block-color",
    "border-inline-width",
    "border-inline-style",
    "border-inline-color",
    "flex-flow",
    "gap",
    "overflow",
    "list-style",
    "place-content",
    "place-items",
    "place-self",
    "white-space",
    "grid-row",
    "grid-column",
    "background",
    "text-wrap",
];

/// A property WebKit supports that the cascade does not compute
/// (`transform`, `cursor`): a declaration block keeps its value as
/// written, and it has its attributes. Not `all`, whose CSS-wide value
/// stands for each computed property. Deliberately unlike a browser,
/// such a value is checked only to be one value (`block/entry.rs`), not
/// against its grammar; it keeps its comments and the case of its
/// function names; and a shorthand of them (`transition`) and its
/// longhands (`transition-duration`) are separate declarations, neither
/// setting nor reading the other.
pub(crate) fn uncomputed(name: &str) -> Option<&'static str> {
    let i = super::supports::KNOWN.binary_search(&name).ok()?;
    let n = super::supports::KNOWN[i];
    (n != "all" && known(n).is_none()).then_some(n)
}

/// The name of a property the cascade computes.
fn known(n: &str) -> Option<&'static str> {
    if let Some(p) = Prop::from_name(n) {
        return Some(p.name());
    }
    LOGICAL.iter().chain(SHORTHANDS).find(|&&k| k == n).copied()
}

/// Every supported property name, each once: longhands, flow-relative
/// longhands, shorthands, the other names, then the properties the
/// cascade does not compute.
pub fn property_names() -> &'static [&'static str] {
    static NAMES: OnceLock<Box<[&'static str]>> = OnceLock::new();
    NAMES.get_or_init(|| {
        Prop::ALL
            .iter()
            .map(|p| p.name())
            .chain(LOGICAL.iter().copied())
            .chain(SHORTHANDS.iter().copied())
            .chain(ALIASES.iter().map(|a| a.0))
            .chain(
                super::supports::KNOWN
                    .iter()
                    .copied()
                    .filter(|n| uncomputed(n).is_some() && !ALIASES.iter().any(|a| a.0 == *n)),
            )
            .collect()
    })
}

/// The name a property is known by, from an ASCII-lowercase name of it;
/// `None` for a name no supported property has.
pub fn canonical(name: &str) -> Option<&'static str> {
    let n = super::decls::alias(name);
    known(n).or_else(|| uncomputed(n))
}

/// The name of a flow-relative longhand.
pub(crate) fn logical_css_name(l: Logical) -> &'static str {
    LOGICAL
        .iter()
        .find(|n| logical_name(n) == Some(l))
        .expect("every flow-relative longhand is listed")
}

/// A shorthand with the longhands it sets, in its order.
pub(crate) struct ShortInfo {
    pub sh: Short,
    pub name: &'static str,
    pub longs: Box<[What]>,
    /// Whether `cssText` writes its longhands as it. Deliberately not:
    /// `background` and `text-wrap`, of whose longhands the cascade keeps
    /// only `background-color` and `text-wrap-mode`, so a block of those
    /// alone does not set the others (and `text-wrap: balance` reads
    /// back `wrap`). `border` is written without `border-image`, which
    /// the cascade does not keep either.
    pub in_css_text: bool,
}

pub(crate) struct Shorts {
    /// Most longhands first, as CSSOM tries them
    pub list: Box<[ShortInfo]>,
    /// The shorthands of each longhand (`slot`), by place in `list`
    by_slot: Box<[Box<[u8]>]>,
}

/// Every computed longhand's own number.
fn slot(w: &What) -> Option<usize> {
    match w {
        What::Prop(p) => Some(*p as usize),
        What::Logical(l) => Some(PROPS + l.base as usize * 6 + l.side as usize),
        What::Custom(_) => None,
    }
}

impl Shorts {
    /// The places in `list` of the shorthands that set `w`.
    pub fn of(&self, w: &What) -> &[u8] {
        slot(w).map_or(&[], |k| &self.by_slot[k])
    }

    pub fn find(&self, sh: Short) -> &ShortInfo {
        self.list
            .iter()
            .find(|s| s.sh == sh)
            .expect("every shorthand is listed")
    }
}

pub(crate) fn shorts() -> &'static Shorts {
    static SHORTS: OnceLock<Shorts> = OnceLock::new();
    SHORTS.get_or_init(|| {
        let mut list: Vec<ShortInfo> = SHORTHANDS
            .iter()
            .map(|&name| {
                let sh = shorthand::by_name(name).expect("a shorthand");
                let in_css_text = !matches!(name, "background" | "text-wrap");
                ShortInfo {
                    sh,
                    name,
                    longs: shorthand::longhands(sh).into(),
                    in_css_text,
                }
            })
            .collect();
        list.sort_by_key(|s| std::cmp::Reverse(s.longs.len()));
        let mut by_slot = vec![Vec::new(); PROPS * 7];
        for (i, s) in list.iter().enumerate() {
            for w in &s.longs {
                by_slot[slot(w).expect("a computed longhand")].push(i as u8);
            }
        }
        Shorts {
            list: list.into(),
            by_slot: by_slot.into_iter().map(Into::into).collect(),
        }
    })
}

/// The shorthand a known name is, if it is one.
pub(crate) fn short(name: &str) -> Option<Short> {
    shorthand::by_name(name)
}
