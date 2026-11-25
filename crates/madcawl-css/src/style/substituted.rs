//! `var()` in the value of an ordinary property, substituted and parsed
//! once per declaration, property and custom property set in a run:
//! the many elements that share a set and match the same rule take the
//! value the first one made.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::Span;

use super::compute::Run;
use super::custom;
use super::props::{self as props, Decl, Prop, Specified};
use super::shorthand;
use super::vals::items;
use super::var::substitute;

/// How many values a run keeps; past it, it starts over.
const MAX: usize = 1 << 14;

/// What a run has substituted, by declaration address, set and
/// property. Declarations stay where they are for the run, but a later
/// run may parse others where they were, so this lives in the run.
#[derive(Default)]
pub(crate) struct Made(RefCell<HashMap<(usize, u32, Prop), Option<Specified>>>);

impl Run<'_, '_> {
    /// The value `d` declares for `p` once `var()` in it is substituted
    /// from `set`; `None` when that makes it invalid. A value made
    /// before in this run spends none of `budget`.
    pub(crate) fn substituted(
        &self,
        p: Prop,
        d: &Decl,
        sets: &[custom::CustomSet],
        set: u32,
        budget: &Cell<usize>,
    ) -> Option<Specified> {
        let key = (d as *const Decl as usize, set, p);
        if let Some(v) = self.made.0.borrow().get(&key) {
            return v.clone();
        }
        let v = substitute_one(p, d, sets, set, budget);
        let mut made = self.made.0.borrow_mut();
        if made.len() >= MAX {
            made.clear();
        }
        made.insert(key, v.clone());
        v
    }
}

fn substitute_one(
    p: Prop,
    d: &Decl,
    sets: &[custom::CustomSet],
    set: u32,
    budget: &Cell<usize>,
) -> Option<Specified> {
    let raw = d.raw.as_deref().expect("the text var() is in");
    let mut get = |n: &str| custom::lookup(sets, set, n);
    let t = substitute(raw, true, &mut get, budget)?;
    let span = Span::new(0, t.len());
    let v = match d.short {
        None => props::parse(p, &t, span)?,
        Some(sh) => {
            let xs = items(&t, span, 64)?;
            let all = shorthand::parse(sh, &t, &xs)?;
            all.into_iter().find(|(w, _)| *w == d.what)?.1
        }
    };
    (v != Specified::Unresolved).then_some(v)
}
