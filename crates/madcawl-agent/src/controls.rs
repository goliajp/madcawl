//! The state of form controls where it differs from their markup: a
//! value typed, a box ticked, an option chosen, files picked. The DOM
//! keeps none of it, as a browser keeps a control's dirty value apart
//! from its `value` attribute. Records read it, so views and diffs show
//! it; a password's value is held here and never written out.

use std::collections::HashMap;
use std::path::PathBuf;

use madcawl_dom::NodeId;

/// The longest value a control holds, in bytes; typing past it is cut.
pub const MAX_VALUE: usize = 1 << 20;

/// A file picked for a file input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picked {
    pub filename: String,
    pub content_type: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Control {
    pub value: Option<String>,
    pub checked: Option<bool>,
    pub selected: Option<bool>,
    pub files: Option<Vec<Picked>>,
}

/// By node; a node's entry goes when the node is freed, so it never
/// holds more entries than the DOM has nodes.
#[derive(Default)]
pub struct Controls {
    map: HashMap<NodeId, Control>,
}

impl Controls {
    pub fn get(&self, n: NodeId) -> Option<&Control> {
        self.map.get(&n)
    }

    pub(crate) fn entry(&mut self, n: NodeId) -> &mut Control {
        self.map.entry(n).or_default()
    }

    pub(crate) fn forget(&mut self, n: NodeId) {
        self.map.remove(&n);
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn value(&self, n: NodeId) -> Option<&str> {
        self.get(n)?.value.as_deref()
    }

    pub fn checked(&self, n: NodeId) -> Option<bool> {
        self.get(n)?.checked
    }

    pub fn selected(&self, n: NodeId) -> Option<bool> {
        self.get(n)?.selected
    }

    pub fn bytes(&self) -> usize {
        let held: usize = self.map.values().map(|c| c.value.as_ref().map_or(0, String::capacity)).sum();
        self.map.capacity() * (size_of::<NodeId>() + size_of::<Control>() + 8) + held
    }
}

/// `v` cut to `MAX_VALUE` bytes at a character boundary.
pub(crate) fn capped(mut v: String) -> String {
    if v.len() > MAX_VALUE {
        let mut at = MAX_VALUE;
        while !v.is_char_boundary(at) {
            at -= 1;
        }
        v.truncate(at);
    }
    v
}
