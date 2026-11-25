//! Custom properties and `var()`.
//!
//! An element's custom properties are a set that never changes once
//! made. An element that declares none shares its parent's set; one
//! that declares some gets a new set of only those, over its parent's.
//! A chain deeper than `MAX_DEPTH` is flattened into one set, so a
//! lookup walks a bounded number of sets. Elements with the same
//! parent set and the same declarations share one set.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use super::cycles;
use super::props::{Decl, Specified, Wide};
use super::rules::NONE;
use super::var::{MAX_LEN, substitute};

const MAX_DEPTH: u8 = 8;

/// How much text substitution may make in one cascade run, all
/// elements together; past it, what is left to substitute is
/// guaranteed-invalid.
pub(crate) const BUDGET: usize = 64 * MAX_LEN;

/// A name and its value, `None` for the guaranteed-invalid value.
type Var = (Box<str>, Option<Box<str>>);

/// A set of custom properties: its own, sorted by name, over its parent's.
/// A name whose value is `None` is the guaranteed-invalid value: it
/// hides the parent's.
pub(crate) struct CustomSet {
    parent: u32,
    depth: u8,
    vars: Box<[Var]>,
}

/// The value of `name` in set `k` (`NONE` for the empty set).
pub(crate) fn lookup<'s>(sets: &'s [CustomSet], mut k: u32, name: &str) -> Option<&'s str> {
    while k != NONE {
        let s = &sets[k as usize];
        if let Ok(i) = s.vars.binary_search_by(|(n, _)| (**n).cmp(name)) {
            return s.vars[i].1.as_deref();
        }
        k = s.parent;
    }
    None
}

/// Every name and value of set `k`, by name, the nearest set's winning.
pub(crate) fn all(sets: &[CustomSet], mut k: u32) -> Vec<(&str, &str)> {
    let mut seen: Vec<(&str, Option<&str>)> = Vec::new();
    while k != NONE {
        let s = &sets[k as usize];
        seen.extend(s.vars.iter().map(|(n, v)| (&**n, v.as_deref())));
        k = s.parent;
    }
    // a stable sort keeps the nearest of each name first
    seen.sort_by(|a, b| a.0.cmp(b.0));
    seen.dedup_by(|farther, nearer| farther.0 == nearer.0);
    seen.into_iter()
        .filter_map(|(n, v)| v.map(|v| (n, v)))
        .collect()
}

#[derive(Clone)]
enum Val<'a> {
    /// A parent set's value, or one as written
    Ref(&'a str),
    Own(Rc<str>),
}

impl AsRef<str> for Val<'_> {
    fn as_ref(&self) -> &str {
        match self {
            Val::Ref(s) => s,
            Val::Own(s) => s,
        }
    }
}

enum State<'a> {
    Todo,
    Busy,
    Done(Option<Val<'a>>),
}

/// An element's declared custom properties resolved over its parent's
/// set: `var()` in them substituted, a cycle making every property in
/// it guaranteed-invalid, fallbacks or not. The cycles are found before
/// any is resolved (`cycles.rs`), so what is invalid does not depend on
/// which property resolving starts from.
struct Resolver<'a> {
    /// Sorted by name, one of each
    declared: &'a [(&'a str, &'a Decl)],
    state: Vec<State<'a>>,
    /// The properties being resolved, outermost first
    stack: Vec<usize>,
    cyclic: Vec<bool>,
    sets: &'a [CustomSet],
    parent: u32,
    budget: &'a Cell<usize>,
}

impl<'a> Resolver<'a> {
    fn get(&mut self, name: &str) -> Option<Val<'a>> {
        match self.declared.binary_search_by(|(n, _)| (*n).cmp(name)) {
            Ok(i) => self.value(i),
            Err(_) => lookup(self.sets, self.parent, name).map(Val::Ref),
        }
    }

    fn value(&mut self, i: usize) -> Option<Val<'a>> {
        if self.cyclic[i] {
            return None;
        }
        match &self.state[i] {
            State::Done(v) => return v.clone(),
            State::Busy => {
                let at = self
                    .stack
                    .iter()
                    .position(|&j| j == i)
                    .expect("busy is stacked");
                for &j in &self.stack[at..] {
                    self.cyclic[j] = true;
                }
                return None;
            }
            State::Todo => {}
        }
        self.state[i] = State::Busy;
        self.stack.push(i);
        let (name, d) = self.declared[i];
        let budget = self.budget;
        let v = match (&d.value, &d.raw) {
            (Specified::Wide(Wide::Initial), _) => None,
            (Specified::Wide(_), _) => lookup(self.sets, self.parent, name).map(Val::Ref),
            (_, Some(raw)) if d.substitutes => {
                substitute(raw, false, &mut |n| self.get(n), budget).map(|v| Val::Own(v.into()))
            }
            (_, raw) => Some(Val::Ref(raw.as_deref().unwrap_or_default())),
        };
        self.stack.pop();
        let v = v.filter(|_| !self.cyclic[i]);
        self.state[i] = State::Done(v.clone());
        v
    }
}

/// What one cascade run keeps from element to element: the sets made
/// in it, by parent set and declarations, and what is left of `BUDGET`.
pub(crate) struct Memo {
    made: HashMap<(u32, Box<[usize]>), u32>,
    pub budget: Cell<usize>,
}

impl Default for Memo {
    fn default() -> Self {
        Memo::new()
    }
}

impl Memo {
    pub fn new() -> Self {
        Memo {
            made: HashMap::new(),
            budget: Cell::new(BUDGET),
        }
    }

    /// A new run of the cascade gets the whole budget again, and
    /// forgets the declarations it made sets of: the run before parsed
    /// style attributes and hints of its own, and a new run may parse
    /// other declarations where those were.
    pub fn new_run(&mut self) {
        self.budget.set(BUDGET);
        self.made.clear();
    }

    /// The set an element gets: its parent's when it declares none.
    /// `declared` is sorted by name, one of each.
    pub fn element_set(
        &mut self,
        sets: &mut Vec<CustomSet>,
        parent: u32,
        declared: &[(&str, &Decl)],
    ) -> u32 {
        if declared.is_empty() {
            return parent;
        }
        // declarations live as long as the run, so their addresses name them
        let ids = declared.iter().map(|(_, d)| *d as *const Decl as usize);
        let key = (parent, ids.collect());
        if let Some(&k) = self.made.get(&key) {
            return k;
        }
        let k = make_set(sets, parent, declared, &self.budget);
        self.made.insert(key, k);
        k
    }
}

fn make_set(
    sets: &mut Vec<CustomSet>,
    parent: u32,
    declared: &[(&str, &Decl)],
    budget: &Cell<usize>,
) -> u32 {
    let mut r = Resolver {
        declared,
        state: declared.iter().map(|_| State::Todo).collect(),
        stack: Vec::new(),
        cyclic: cycles::cyclic(declared),
        sets,
        parent,
        budget,
    };
    // in name order already, as `declared` is
    let vars: Vec<Var> = (0..declared.len())
        .map(|i| (declared[i].0.into(), r.value(i).map(|v| v.as_ref().into())))
        .collect();
    let depth = if parent == NONE {
        0
    } else {
        sets[parent as usize].depth + 1
    };
    let set = if depth < MAX_DEPTH {
        CustomSet {
            parent,
            depth,
            vars: vars.into(),
        }
    } else {
        let mut flat: Vec<Var> = all(sets, parent)
            .into_iter()
            .filter(|(n, _)| vars.binary_search_by(|(m, _)| (**m).cmp(n)).is_err())
            .map(|(n, v)| (n.into(), Some(v.into())))
            .collect();
        flat.extend(vars.into_iter().filter(|(_, v)| v.is_some()));
        flat.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        CustomSet {
            parent: NONE,
            depth: 0,
            vars: flat.into(),
        }
    };
    sets.push(set);
    sets.len() as u32 - 1
}
