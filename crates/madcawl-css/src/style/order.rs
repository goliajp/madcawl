//! The order of the cascade: each declaration's place as one number,
//! and the winning declaration of each property among those that apply
//! to an element.

use madcawl_dom::NodeId;
use madcawl_str::atom;

use super::compute::{Run, Src};
use super::computed::ComputedStyle;
use super::kw::{Direction, WritingMode};
use super::logical::{Logical, physical};
use super::matching::text;
use super::props::{Decl, PROPS, Prop, Specified, What, Wide};

/// A declared value's place in the cascade, compared as one number:
/// origin and importance, element-attached, layer, specificity, order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Key(u128);

impl Key {
    pub(crate) fn new(
        origin: u32,
        important: bool,
        attached: bool,
        layer: u32,
        spec: u32,
        order: u32,
    ) -> Key {
        let level = if important { 6 - origin } else { origin + 1 };
        let layer = if important { !layer } else { layer };
        Key(u128::from(level) << 100
            | u128::from(attached) << 96
            | u128::from(layer) << 64
            | u128::from(spec) << 32
            | u128::from(order))
    }

    fn level(self) -> u32 {
        (self.0 >> 100) as u32
    }

    fn origin(self) -> u32 {
        let l = self.level();
        if l <= 3 { l - 1 } else { 6 - l }
    }

    /// Element-attached and layer, as one number
    fn layer(self) -> u64 {
        (self.0 >> 64) as u64 & ((1 << 33) - 1)
    }
}

/// What a `revert` or `revert-layer` already seen leaves a property to.
#[derive(Clone, Copy)]
enum Floor {
    /// Only origins below this one
    Origin(u32),
    /// Only lower layers at this level, or lower levels
    Layer { level: u32, layer: u64 },
}

impl Floor {
    fn allows(self, k: Key) -> bool {
        match self {
            Floor::Origin(o) => k.origin() < o,
            Floor::Layer { level, layer } => k.level() != level || k.layer() < layer,
        }
    }
}

pub(crate) type Won<'d> = [Option<&'d Decl>; PROPS];

pub(crate) struct Cascaded<'d> {
    pub won: Won<'d>,
    pub customs: Vec<(&'d str, &'d Decl)>,
    /// Some declaration was flow-relative
    pub logical: bool,
}

impl Run<'_, '_> {
    /// The winning declaration of each property, and of each custom
    /// property by name, among those `collect` found. A flow-relative
    /// declaration sets the side `map` gives it, or none.
    fn winners(&self, el: NodeId, map: &dyn Fn(Logical) -> Option<Prop>) -> Cascaded<'_> {
        let mut won: Won<'_> = [None; PROPS];
        let mut floor: [Option<Floor>; PROPS] = [None; PROPS];
        let mut customs: Vec<(&str, &Decl)> = Vec::new();
        let mut logical = false;
        let style = self.m.dom.attribute(el, atom!("style")).map(|s| text(s));
        for (key, important, src) in &self.matched {
            let decls = match src {
                Src::Block(b) => self.m.rules.blocks[*b as usize].get(),
                Src::Inline => &self.inline[style.as_deref().expect("a style attribute")],
                Src::Hint => &self.hinted[self.hint.as_deref().expect("hints")],
            };
            for d in decls.iter().rev().filter(|d| d.important == *important) {
                let p = match &d.what {
                    What::Prop(p) => *p as usize,
                    What::Logical(l) => {
                        logical = true;
                        match map(*l) {
                            Some(p) => p as usize,
                            None => continue,
                        }
                    }
                    What::Custom(name) => {
                        if !customs.iter().any(|(n, _)| *n == &**name) {
                            customs.push((name, d));
                        }
                        continue;
                    }
                };
                if won[p].is_some() || floor[p].is_some_and(|f| !f.allows(*key)) {
                    continue;
                }
                match d.value {
                    Specified::Wide(Wide::Revert) => floor[p] = Some(Floor::Origin(key.origin())),
                    Specified::Wide(Wide::RevertLayer) => {
                        floor[p] = Some(Floor::Layer {
                            level: key.level(),
                            layer: key.layer(),
                        })
                    }
                    _ => won[p] = Some(d),
                }
            }
        }
        Cascaded {
            won,
            customs,
            logical,
        }
    }

    /// `winners`, the flow-relative declarations mapped by the element's
    /// own writing mode and direction, which the physical ones decide.
    pub(crate) fn cascade(&self, el: NodeId, parent: &ComputedStyle) -> Cascaded<'_> {
        let first = self.winners(el, &|_| None);
        if !first.logical {
            return first;
        }
        let pick = |p: Prop| match first.won[p as usize].map(|d| &d.value) {
            Some(Specified::Wide(Wide::Initial)) => Some(None),
            Some(v @ (Specified::WritingMode(_) | Specified::Direction(_))) => Some(Some(v)),
            _ => None,
        };
        let wm = match pick(Prop::WritingMode) {
            Some(Some(Specified::WritingMode(w))) => *w,
            Some(_) => WritingMode::HorizontalTb,
            None => parent.text.writing_mode,
        };
        let dir = match pick(Prop::Direction) {
            Some(Some(Specified::Direction(d))) => *d,
            Some(_) => Direction::Ltr,
            None => parent.text.direction,
        };
        self.winners(el, &|l| Some(physical(l, wm, dir)))
    }
}
