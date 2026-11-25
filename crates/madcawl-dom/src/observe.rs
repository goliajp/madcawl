//! A record of the tree's changes for one observer outside the DOM, such
//! as the semantic tree an agent reads. Nothing is recorded, and nothing
//! is allocated, until something asks to observe; the record is bounded,
//! and once it overflows the observer is told to read the tree again.

use crate::dom::{Dom, NodeId};
use crate::tree::Change;

/// A change to the tree, as an observer reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutation {
    /// `NodeId` was put into a parent, or moved
    Inserted(NodeId),
    /// `child` was taken out of `parent`; it may still be alive
    Removed { parent: NodeId, child: NodeId },
    /// An attribute of the element was set
    Attribute(NodeId),
    /// The data of a text, comment or processing instruction node
    Text(NodeId),
    /// The node's slot was freed; a later node may take it
    Freed(NodeId),
}

pub(crate) struct Observer {
    log: Vec<Mutation>,
    cap: usize,
    overflowed: bool,
}

impl Dom {
    /// Starts recording changes, at most `cap` between two
    /// `take_mutations`. An observer already there keeps its record.
    pub fn observe(&mut self, cap: usize) {
        assert!(cap > 0, "room for one change");
        if self.observer.is_none() {
            self.observer = Some(Box::new(Observer {
                log: Vec::new(),
                cap,
                overflowed: false,
            }));
        }
    }

    /// Stops recording and drops the record.
    pub fn unobserve(&mut self) {
        self.observer = None;
    }

    pub fn is_observed(&self) -> bool {
        self.observer.is_some()
    }

    /// Moves the changes since the last call into `out`, in order; false
    /// when more happened than the record holds, and the observer must
    /// read the whole tree again (`out` then gets nothing).
    pub fn take_mutations(&mut self, out: &mut Vec<Mutation>) -> bool {
        let Some(o) = self.observer.as_mut() else {
            return true;
        };
        let whole = !o.overflowed;
        if whole {
            out.append(&mut o.log);
        }
        o.log.clear();
        o.overflowed = false;
        whole
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn record(&mut self, change: &Change) {
        let Some(o) = self.observer.as_mut() else {
            return;
        };
        if o.overflowed {
            return;
        }
        if o.log.len() == o.cap {
            o.overflowed = true;
            o.log = Vec::new();
            return;
        }
        o.log.push(match *change {
            Change::Inserted(n) => Mutation::Inserted(NodeId::at(n)),
            Change::Removed { parent, child } => Mutation::Removed {
                parent: NodeId::at(parent),
                child: NodeId::at(child),
            },
            Change::Attributes(n) => Mutation::Attribute(NodeId::at(n)),
            Change::Text(n) => Mutation::Text(NodeId::at(n)),
            Change::Freed(n) => Mutation::Freed(NodeId::at(n)),
        });
    }
}

#[cfg(test)]
mod tests {
    use madcawl_str::{DomString, atom};

    use super::*;

    #[test]
    fn nothing_is_recorded_or_allocated_without_an_observer() {
        let mut dom = Dom::new();
        let el = dom.new_element(b"div").unwrap();
        dom.insert_node(dom.document(), el, None);
        assert!(!dom.is_observed());
        let mut out = Vec::new();
        assert!(dom.take_mutations(&mut out));
        assert!(out.is_empty());
    }

    #[test]
    fn changes_come_in_order_and_an_overflow_says_read_again() {
        let mut dom = Dom::new();
        dom.observe(3);
        let doc = dom.document();
        let el = dom.new_element(b"p").unwrap();
        dom.insert_node(doc, el, None);
        dom.set_attribute(el, atom!("id"), b"x");
        let t = dom.new_text(DomString::from_wtf8(b"a")).unwrap();
        dom.insert_node(el, t, None);
        let mut out = Vec::new();
        assert!(dom.take_mutations(&mut out));
        assert_eq!(
            out,
            vec![
                Mutation::Inserted(el),
                Mutation::Attribute(el),
                Mutation::Inserted(t)
            ]
        );
        out.clear();
        dom.set_text(t, DomString::from_wtf8(b"b"));
        dom.remove_node(t);
        assert!(dom.take_mutations(&mut out));
        assert_eq!(
            out,
            vec![
                Mutation::Text(t),
                Mutation::Removed {
                    parent: el,
                    child: t
                }
            ]
        );
        out.clear();
        for _ in 0..4 {
            dom.set_attribute(el, atom!("title"), b"y");
        }
        assert!(
            !dom.take_mutations(&mut out),
            "four changes, room for three"
        );
        assert!(out.is_empty());
        dom.set_attribute(el, atom!("title"), b"z");
        assert!(dom.take_mutations(&mut out), "recording again");
        assert_eq!(out, vec![Mutation::Attribute(el)]);
        dom.unobserve();
        assert!(!dom.is_observed());
    }
}
