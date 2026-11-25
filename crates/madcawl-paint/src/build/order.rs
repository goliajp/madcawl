//! Sorting a stacking context's descendants into the layers it paints them in.
//!
//! An out-of-flow box's fragment hangs under its containing block's fragment (the root's for a fixed
//! box), so the clips met on the way down the fragment tree are those of the boxes it is clipped by.

use std::sync::Arc;

use madcawl_css::style::{ComputedStyle, Float, Position};
use madcawl_layout::{FragKind, Fragment, Point};

use super::Painter;

/// What a box is painted under: the clip node and the opacity of its ancestors.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub clip: u32,
    pub alpha: f32,
}

/// A fragment with its place on the page; for a fragment on a line, that line and its place too.
#[derive(Clone, Debug)]
pub struct Unit {
    pub frag: Arc<Fragment>,
    pub at: Point,
    pub cx: Ctx,
    pub line: Option<(Arc<Fragment>, Point)>,
}

/// A stacking context's descendants by layer, each in tree order; those with a `z-index` with it.
#[derive(Default)]
pub struct Layers {
    pub negative: Vec<(i32, Unit)>,
    pub blocks: Vec<Unit>,
    pub floats: Vec<Unit>,
    pub lines: Vec<Unit>,
    /// Positioned boxes with `z-index: auto` and stacking contexts at 0
    pub flow: Vec<Unit>,
    pub positive: Vec<(i32, Unit)>,
}

impl Layers {
    fn layer(&mut self, z: i32, u: Unit) {
        match z {
            ..0 => self.negative.push((z, u)),
            0 => self.flow.push(u),
            _ => self.positive.push((z, u)),
        }
    }

    /// Orders the stacking contexts by `z-index`, tree order among equals.
    pub fn sort(&mut self) {
        self.negative.sort_by_key(|c| c.0);
        self.positive.sort_by_key(|c| c.0);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Class {
    /// An anonymous box: only its children paint
    Anon,
    /// A stacking context at this `z-index`: positioned with a `z-index`, or translucent
    Context(i32),
    /// Positioned with `z-index: auto`: it paints as if it made a stacking context, but its positioned
    /// descendants and the stacking contexts in it belong to the enclosing one
    Positioned,
    Float,
    Block,
}

impl Class {
    /// Painted in a layer of a stacking context rather than with the content it sits in.
    pub(super) fn layered(self) -> bool {
        matches!(self, Class::Context(_) | Class::Positioned)
    }
}

pub(super) fn class(s: Option<&ComputedStyle>) -> Class {
    let Some(s) = s else { return Class::Anon };
    let positioned = s.position != Position::Static;
    if positioned && let Some(z) = s.place.z_index {
        return Class::Context(z);
    }
    if s.opacity() < 1.0 {
        return Class::Context(0);
    }
    if positioned {
        return Class::Positioned;
    }
    if s.float != Float::None {
        return Class::Float;
    }
    Class::Block
}

/// How far a walk takes what it meets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Walk {
    /// The contents of a stacking context: everything goes into its layers
    Context,
    /// The contents of a box painting as if it made a stacking context, seen from the stacking context
    /// around it: only the positioned boxes and stacking contexts are taken
    Escape,
    /// The contents of a box painting as if it made a stacking context, as it paints them: the positioned
    /// boxes and stacking contexts are left to the stacking context around it
    Own,
}

fn children(u: &Unit, cx: Ctx, w: Walk, stack: &mut Vec<(Unit, Walk)>) {
    let from = stack.len();
    let line = (u.frag.kind == FragKind::Line).then(|| (u.frag.clone(), u.at));
    stack.extend(u.frag.children.iter().map(|c| {
        let u = Unit {
            frag: c.frag.clone(),
            at: u.at + c.offset,
            cx,
            line: line.clone(),
        };
        (u, w)
    }));
    stack[from..].reverse();
}

impl Painter<'_, '_, '_> {
    pub(super) fn class_of(&self, f: &Fragment) -> Class {
        class(f.node.and_then(|n| self.style(n)))
    }

    /// Sorts the descendants of `top`, whose contents are painted under `cx`, into `l`, walking as `w`
    /// says; stacking contexts are not looked into.
    pub(super) fn gather(&mut self, top: &Unit, cx: Ctx, w: Walk, l: &mut Layers) {
        let mut stack = Vec::new();
        children(top, cx, w, &mut stack);
        while let Some((u, w)) = stack.pop() {
            match u.frag.kind {
                FragKind::Text => {}
                FragKind::Line => {
                    if w != Walk::Escape {
                        l.lines.push(u.clone());
                    }
                    if w != Walk::Own {
                        children(&u, u.cx, w, &mut stack);
                    }
                }
                FragKind::Box if u.line.is_some() => self.on_line(u, l, &mut stack),
                FragKind::Box => self.in_flow(u, w, l, &mut stack),
            }
        }
    }

    // a box on a line, met by a walk that is not `Own`: the line paints it unless it is layered; what is
    // in an atomic inline that makes no stacking context is looked into for what escapes it
    fn on_line(&mut self, u: Unit, l: &mut Layers, stack: &mut Vec<(Unit, Walk)>) {
        let class = self.class_of(&u.frag);
        if !u.frag.children.is_empty() && !matches!(class, Class::Context(_)) {
            let inner = self.inner(&u.frag, u.at, u.cx);
            children(&u, inner, Walk::Escape, stack);
        }
        if !class.layered() {
            return;
        }
        let mut u = u;
        if let Some((line, _)) = &u.line
            && u.frag.children.is_empty()
        {
            // a piece of an inline box: the opacity of the inline boxes it is in reaches it
            u.cx.alpha *= self.chain_alpha(line, u.frag.node.and_then(|n| self.dom.parent(n)));
        }
        match class {
            Class::Context(z) => l.layer(z, u),
            _ => l.flow.push(u),
        }
    }

    fn in_flow(&mut self, u: Unit, w: Walk, l: &mut Layers, stack: &mut Vec<(Unit, Walk)>) {
        let class = self.class_of(&u.frag);
        match (class, w) {
            (Class::Anon, _) => children(&u, u.cx, w, stack),
            (Class::Context(z), Walk::Context | Walk::Escape) => l.layer(z, u),
            (Class::Positioned, Walk::Context | Walk::Escape) => {
                let inner = self.inner(&u.frag, u.at, u.cx);
                children(&u, inner, Walk::Escape, stack);
                l.flow.push(u);
            }
            (Class::Context(_) | Class::Positioned, Walk::Own) => {}
            (Class::Float, _) => {
                if w != Walk::Own {
                    let inner = self.inner(&u.frag, u.at, u.cx);
                    children(&u, inner, Walk::Escape, stack);
                }
                if w != Walk::Escape {
                    l.floats.push(u);
                }
            }
            (Class::Block, _) => {
                let inner = self.inner(&u.frag, u.at, u.cx);
                children(&u, inner, w, stack);
                if w != Walk::Escape {
                    l.blocks.push(u);
                }
            }
        }
    }
}
